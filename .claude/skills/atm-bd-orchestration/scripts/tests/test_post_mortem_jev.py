import copy,json,subprocess,sys,tempfile,unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parents[1]))
import post_mortem_jev as m
REAL=m.load_client(Path(__file__).resolve().parents[5] / 'scripts/jev_client.py')

def response():
 answers={}
 for key,q in m.QUESTIONS.items():
  choice={'fix_presence':'fully_present','serious_fix_issues':'no_serious_issue_seen','fix_quality':'adequate'}[key]
  answers[key]={'type':'choice','choice':choice,'confidence':.9,'probabilities':{k:(1.0 if k==choice else 0.0) for k in q['criteria']}}
 return {'model':REAL.MODEL,'answers':answers}
class FakeClient:
 MODEL=REAL.MODEL
 validate_request=staticmethod(REAL.validate_request)
 @staticmethod
 def evaluate(req):return response()
class Tests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name);self.repo=self.root/'repo';self.repo.mkdir()
  for args in [('init','-q'),('config','user.name','test'),('config','user.email','test@example.invalid')]:subprocess.run(['git','-C',str(self.repo),*args],check=True,capture_output=True)
  (self.repo/'code.rs').write_text('fn execute() {}\n#[cfg(test)]\nmod tests { fn execute_test() {} }\n')
  subprocess.run(['git','-C',str(self.repo),'add','.'],check=True);subprocess.run(['git','-C',str(self.repo),'commit','-qm','fixture'],check=True)
  self.head=m.resolve_sha(self.repo,'HEAD');self.cover={'scope':'whole_finding','limitations':[],'obligation_ids':['o1']}
  self.man={'finding_id':'f1','phase':'phase-d','integration_sha':self.head,'finding_text':'A complete original defect.','acceptance_predicates':[{'id':'o1','text':'execute exists'}],'coverage':self.cover,'source_selections':[{'path':'code.rs','role':'mixed'}]}
 def packet(self):return m.prepare(self.man,self.repo,FakeClient)
 def save(self,name,obj):
  p=self.root/name;p.write_text(json.dumps(obj));return p
 def test_exact_path_missing_does_not_fallback(self):
  self.man['source_selections'][0]['path']='another/code.rs'
  with self.assertRaises(ValueError):self.packet()
 def test_invalid_range_does_not_clip(self):
  self.man['source_selections'][0]['end_line']=999
  with self.assertRaises(ValueError):self.packet()
 def test_role_and_complete_file_retained(self):
  p=self.packet();self.assertEqual(p['state']['current_evidence'][0]['role'],'mixed');self.assertTrue(p['state']['current_evidence'][0]['complete_file']);self.assertIn('#[cfg(test)]',p['state']['current_evidence'][0]['text'])
 def test_negative_search_is_scoped(self):
  self.man['searches']=[{'literal':'absent_symbol','paths':['code.rs']}]
  s=self.packet()['state']['searches'][0];self.assertEqual(s['exit_code'],1);self.assertEqual(s['matches'],'');self.assertIn('does not establish behavioral absence',s['interpretation'])
 def test_oversize_does_not_truncate(self):
  self.man['finding_text']='x'*25000
  with self.assertRaises(REAL.JevError):self.packet()
 def test_obligation_mismatch_rejected(self):
  self.man['coverage']['obligation_ids']=['o2']
  with self.assertRaises(ValueError):self.packet()
 def test_immutable_snapshot_survives_input_rewrite(self):
  p=self.save('packet.json',self.packet());raw=p.read_bytes();r=m.run([p],FakeClient,self.root/'logs','phase-d')[0];p.write_text('changed')
  self.assertEqual(Path(r['packet_path']).read_bytes(),raw);self.assertEqual(m.digest(raw),r['packet_sha256']);self.assertEqual(r['screening_disposition'],'screened_present');self.assertTrue(r['timestamp_utc'].endswith('Z'));self.assertNotIn('local_time',r)
 def test_bad_inputs_log_errors_and_continue(self):
  good=self.save('good.json',self.packet());bad=self.root/'bad.json';bad.write_text('{')
  rows=m.run([bad,self.root/'missing.json',good],FakeClient,self.root/'logs','phase-d',concurrency=3)
  self.assertEqual(len(rows),3);self.assertEqual(sum(r['error'] is not None for r in rows),2);self.assertEqual(len((self.root/'logs/post-mortem-jev-phase-d.jsonl').read_text().splitlines()),3)
 def test_controls_rejected_and_not_sent(self):
  obj=self.packet();obj['state']['evaluation_kind']='calibration';r=m.run([self.save('control.json',obj)],FakeClient,self.root/'logs','phase-d')[0]
  self.assertIsNotNone(r['error']);self.assertIsNone(r['response'])
 def test_subcheck_never_counts_whole(self):
  c=copy.deepcopy(self.cover);c['scope']='subcheck';self.assertEqual(m.route(response(),c),'needs_context')
 def test_limitations_and_low_probability_need_context(self):
  c=copy.deepcopy(self.cover);c['limitations']=['caller omitted'];self.assertEqual(m.route(response(),c),'needs_context')
  r=response();r['answers']['fix_presence']['probabilities']['fully_present']=.7;self.assertEqual(m.route(r,self.cover),'needs_context')
 def test_flag_is_inspection_candidate_not_confirmed_issue(self):
  r=response();r['answers']['serious_fix_issues']['choice']='serious_issue';self.assertEqual(m.route(r,self.cover),'inspect_candidate')
 def test_initial_repair_counts_separate(self):
  rows=[{'run_id':'r','attempt_role':a,'screening_disposition':d} for a,d in [('initial','needs_context'),('context_repair','screened_present')]]
  self.assertEqual(len(m.summarize(rows)),2)
 def test_concurrent_repeated_packet_keeps_complete_artifact(self):
  p=self.save('same.json',self.packet());rows=m.run([p]*8,FakeClient,self.root/'logs','phase-d',concurrency=4)
  self.assertTrue(all(r['error'] is None for r in rows));self.assertEqual(len(set(r['packet_path'] for r in rows)),1)
  self.assertEqual(Path(rows[0]['packet_path']).read_bytes(),p.read_bytes())
 def test_phase_mismatch_logged(self):
  obj=self.packet();obj['state']['phase']='phase-e';r=m.run([self.save('wrong.json',obj)],FakeClient,self.root/'logs','phase-d')[0];self.assertIsNotNone(r['error'])
if __name__=='__main__':unittest.main()
