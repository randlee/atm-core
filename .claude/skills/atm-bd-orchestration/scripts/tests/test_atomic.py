import copy
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parents[1]))
import test_post_mortem_jev as legacy
FakeClient = legacy.FakeClient
import post_mortem_jev as m

Q={'type':'choice','instructions':'Does the deciding code establish this behavior?','criteria':{'yes':'It does.','no':'It establishes the opposite.','insufficient':'Deciding evidence is absent or ambiguous.'}}
def answers(choices):
 return {'answers':{k:{'choice':c,'probabilities':{c:p},'confidence':p} for k,(c,p) in choices.items()}}
class Atomic(unittest.TestCase):
 def setUp(self):
  self.p=[{'id':'o1','text':'Required behavior'}]
  self.c={'scope':'whole_finding','limitations':[],'obligation_ids':['o1']}
  self.s={'behavior':{'category':'presence','expected':'yes','obligation_ids':['o1']}}
 def agg(self,a):return m.aggregate_atomic(answers(a),self.c,self.s,self.p)
 def test_expected_no_is_supported(self):
  self.s['behavior']['expected']='no'
  self.assertEqual(self.agg({'behavior':('no',.95)})['presence_disposition'],'supported')
 def test_negative_is_not_automatically_defect(self):
  self.s['behavior']['expected']='no'
  self.assertEqual(self.agg({'behavior':('no',.95)})['screening_disposition'],'screened_present')
 def test_low_probability_quality_separate(self):
  self.s['rank']={'category':'quality','attention_choices':['broken']}
  r=self.agg({'behavior':('yes',.95),'rank':('adequate',.4)})
  self.assertEqual(r['screening_disposition'],'screened_present');self.assertEqual(r['uncertain_question_ids'],['rank']);self.assertTrue(r['investigation_required']);self.assertEqual(r['quality_disposition'],'needs_context')
 def test_issue_flag_keeps_presence_separate(self):
  self.s['unsafe']={'category':'issue','expected':'no','obligation_ids':['o1']}
  r=self.agg({'behavior':('yes',.95),'unsafe':('yes',.55)})
  self.assertEqual(r['presence_disposition'],'supported');self.assertEqual(r['issue_disposition'],'flagged');self.assertEqual(r['screening_disposition'],'inspect_candidate')
 def test_low_issue_requires_investigation_without_erasing_presence(self):
  self.s['unsafe']={'category':'issue','expected':'no','obligation_ids':['o1']}
  r=self.agg({'behavior':('yes',.95),'unsafe':('no',.5)})
  self.assertEqual(r['presence_disposition'],'supported');self.assertTrue(r['investigation_required']);self.assertEqual(r['issue_disposition'],'needs_context')
 def test_presence_only_does_not_claim_quality_review(self):
  r=self.agg({'behavior':('yes',.95)})
  self.assertEqual(r['issue_disposition'],'not_evaluated');self.assertEqual(r['quality_disposition'],'not_evaluated');self.assertFalse(r['investigation_required'])
 def test_subcheck_never_whole(self):
  self.c['scope']='subcheck'
  self.assertEqual(self.agg({'behavior':('yes',.95)})['presence_disposition'],'needs_context')
 def test_insufficient_never_supported(self):
  self.assertEqual(self.agg({'behavior':('insufficient',.99)})['presence_disposition'],'needs_context')
 def test_weak_negative_needs_context(self):
  self.assertEqual(self.agg({'behavior':('no',.54)})['screening_disposition'],'needs_context')
 def test_every_obligation_required(self):
  self.p.append({'id':'o2','text':'Other behavior'});self.c['obligation_ids'].append('o2')
  with self.assertRaises(ValueError):m.validate_question_specs({'behavior':Q},self.s,self.c,self.p)
 def test_no_implicit_polarity(self):
  del self.s['behavior']['expected']
  with self.assertRaises(ValueError):m.validate_question_specs({'behavior':Q},self.s,self.c,self.p)
 def test_unknown_question_mapping_rejected(self):
  with self.assertRaises(ValueError):m.validate_question_specs({'other':Q},self.s,self.c,self.p)

class Pipeline(unittest.TestCase):
 setUp = legacy.Tests.setUp
 save = legacy.Tests.save
 def test_v2_roundtrip_and_prebuilt_validation(self):
  self.man['questions']={'behavior':Q}
  self.man['question_specs']={'behavior':{'category':'presence','expected':'no','obligation_ids':['o1']}}
  class Client(FakeClient):
   @staticmethod
   def evaluate(req):
    assert 'question_specs' not in req['state'], 'answer key leaked to evaluator'
    return answers({'behavior':('no',.95)})
  packet=m.prepare(self.man,self.repo,Client)
  row=m.run([self.save('atomic.json',packet)],Client,self.root/'logs','phase-d')[0]
  self.assertIsNone(row['error']);self.assertEqual(row['schema_version'],2);self.assertEqual(row['presence_disposition'],'supported')
  packet['state']['question_specs']['behavior']['obligation_ids']=['not-real']
  row=m.run([self.save('bad-atomic.json',packet)],Client,self.root/'logs','phase-d')[0]
  self.assertIsNotNone(row['error']);self.assertIsNone(row['response'])
if __name__=='__main__':unittest.main()
