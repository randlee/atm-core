use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn read_manifest() -> Value {
    let path = repo_root().join("docs/user-documents/examples/cron/manifest.json");
    serde_json::from_slice(&fs::read(path).expect("read cron fixtures"))
        .expect("parse cron fixture manifest")
}

fn fixture_blocks(path: &Path) -> BTreeMap<String, (String, String)> {
    let text = fs::read_to_string(path).expect("read fixture documentation");
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(id) = lines[index]
            .trim()
            .strip_prefix("<!-- fixture: ")
            .and_then(|value| value.strip_suffix(" -->"))
        else {
            index += 1;
            continue;
        };
        index += 1;
        while index < lines.len() && lines[index].trim().is_empty() {
            index += 1;
        }
        let fence = lines
            .get(index)
            .and_then(|line| line.trim().strip_prefix("\x60\x60\x60"))
            .unwrap_or_else(|| panic!("fixture marker {id} is not followed by a code fence"));
        let language = fence.trim().to_owned();
        index += 1;
        let mut body = Vec::new();
        while index < lines.len() && lines[index].trim() != "\x60\x60\x60" {
            body.push(lines[index]);
            index += 1;
        }
        assert!(
            index < lines.len(),
            "fixture {id} has an unclosed code fence"
        );
        assert!(
            blocks
                .insert(id.to_owned(), (language, body.join("\n")))
                .is_none(),
            "duplicate fixture marker {id}"
        );
        index += 1;
    }
    blocks
}

fn assert_every_fence_is_marked(document: &str, section: &str) {
    let mut in_fence = false;
    let lines: Vec<&str> = document.lines().collect();
    for (line_number, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            if !in_fence {
                let marker = lines
                    .iter()
                    .take(line_number)
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim());
                assert!(
                    marker.is_some_and(|line| {
                        line.starts_with("<!-- fixture: ") && line.ends_with(" -->")
                    }),
                    "unmarked fenced example in {section} at line {}",
                    line_number + 1
                );
                in_fence = true;
            } else {
                in_fence = false;
            }
        }
    }
    assert!(!in_fence, "unclosed fenced example in {section}");
}

#[test]
fn cron_doc_blocks_match_manifest() {
    let root = repo_root();
    let manifest = read_manifest();
    let cron_doc = fs::read_to_string(root.join("docs/user-documents/cron.md")).unwrap();
    assert_every_fence_is_marked(&cron_doc, "cron.md");
    let hooks_doc = fs::read_to_string(root.join("docs/user-documents/hooks.md")).unwrap();
    let hook_contract = hooks_doc
        .split_once("## Hook mutation JSON contract")
        .unwrap()
        .1
        .split_once("## Cron")
        .unwrap()
        .0;
    assert_every_fence_is_marked(hook_contract, "hooks.md hook contract");
    let nudge_doc =
        fs::read_to_string(root.join("docs/user-documents/nudge-templates.md")).unwrap();
    let cron_nudges = nudge_doc
        .split_once("## Cron lifecycle notices")
        .unwrap()
        .1
        .split_once("## Default ATM Wire Bodies")
        .unwrap()
        .0;
    assert_every_fence_is_marked(cron_nudges, "nudge-templates.md cron section");
    let mut observed = BTreeSet::new();
    for example in manifest["examples"].as_array().expect("examples array") {
        let id = example["id"].as_str().expect("example id");
        let file = example["file"].as_str().expect("example file");
        let blocks = fixture_blocks(&root.join("docs/user-documents").join(file));
        let (language, body) = blocks
            .get(id)
            .unwrap_or_else(|| panic!("missing fixture block {id} in {file}"));
        assert_eq!(
            language,
            example["language"].as_str().unwrap(),
            "fixture {id}"
        );
        assert_eq!(
            body.trim_end(),
            example["text"].as_str().expect("fixture text").trim_end(),
            "documented fixture {id} drifted from the manifest"
        );
        assert!(observed.insert((file.to_owned(), id.to_owned())));
    }
    for file in ["cron.md", "hooks.md", "nudge-templates.md"] {
        for id in fixture_blocks(&root.join("docs/user-documents").join(file)).keys() {
            assert!(
                observed.contains(&(file.to_owned(), id.clone())),
                "unmanifested fixture marker {id} in {file}"
            );
        }
    }
}

#[test]
fn cron_and_hook_contract_fixtures_have_frozen_shapes() {
    let manifest = read_manifest();
    let commands = manifest["commands"].as_array().expect("commands array");
    for id in ["reject-foreign-from", "reject-malformed-output"] {
        let fixture = commands
            .iter()
            .find(|fixture| fixture["id"] == id)
            .unwrap_or_else(|| panic!("missing rejection fixture {id}"));
        assert_eq!(fixture["classification"], "rejection");
        assert_eq!(fixture["deferred_execution"], true);
    }

    for reply in manifest["replies"].as_array().expect("replies array") {
        let reply_id = reply["id"].as_str().expect("reply id");
        let json_value = &reply["json"];
        let hook = reply["hook"].as_str().expect("reply hook");
        if hook == "cron.stop" {
            let keys = json_value
                .as_object()
                .expect("stop reply object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert!(keys.contains("stop"), "{reply_id}");
            assert!(
                keys.iter()
                    .all(|key| ["stop", "reason", "cron-output"].contains(key))
            );
            if json_value["stop"] == true {
                assert!(json_value["reason"].as_str().is_some(), "{reply_id}");
            } else {
                assert!(json_value.get("reason").is_none(), "{reply_id}");
            }
        } else {
            let object = json_value.as_object().expect("tick reply object");
            let result = object["result"].as_str().expect("tick result");
            assert!(["pass-through", "handled", "not-applicable", "refused"].contains(&result));
            assert!(
                object.keys().all(|key| {
                    ["result", "atm-action", "cron-output", "message"].contains(&key.as_str())
                }),
                "unknown frozen tick reply key in {reply_id}"
            );
            match result {
                "pass-through" => assert!(object.get("atm-action").is_none()),
                "handled" => assert!(object["atm-action"].as_array().is_some()),
                "not-applicable" => assert_eq!(object.len(), 1),
                "refused" => assert!(object["message"].as_str().is_some()),
                _ => unreachable!(),
            }
        }
        if let Some(outputs) = json_value.get("cron-output") {
            for output in outputs.as_array().expect("cron-output array") {
                let item = output.as_object().expect("cron-output object");
                assert_eq!(item.len(), 2);
                assert!(["message", "error"].contains(&item["kind"].as_str().unwrap()));
                assert!(!item["body"].as_str().unwrap().is_empty());
            }
        }
        if let Some(outputs) = reply.get("outputs").and_then(Value::as_array) {
            for output in outputs {
                assert_eq!(output["sender"], "cron@tool");
                assert_eq!(output["recipient"], "creator@atm-dev");
                assert_eq!(output["category"], "cron");
                let kind = output["tag"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("output:")
                    .unwrap();
                assert!(["message", "error"].contains(&kind));
                assert_eq!(output["body"], json_value["cron-output"][0]["body"]);
            }
        }
    }

    let registration = &manifest["registration"];
    let registration_fields = [
        "team",
        "phase",
        "hook",
        "module_path",
        "cwd",
        "timeout_ms",
        "custom",
        "active",
        "registered_by",
        "registered_at",
        "disabled_at",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let register = registration["register"]["registration"]
        .as_object()
        .expect("registration row");
    assert_eq!(
        register.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        registration_fields
    );
    for (name, result) in registration.as_object().unwrap() {
        if name == "register" {
            assert_eq!(result["action"], "hook.register");
        } else {
            assert_eq!(result["action"], "hook.disable");
            let rows = result["disabled"].as_array().expect("disabled rows");
            assert_eq!(result["count"], rows.len());
            for row in rows {
                assert_eq!(
                    row.as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>(),
                    registration_fields
                );
            }
        }
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create destination");
    for entry in fs::read_dir(source).expect("read source tree") {
        let entry = entry.expect("directory entry");
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).expect("copy installed file");
        }
    }
}

fn run_help(executable: &Path, args: &[&str]) -> String {
    let temp = TempDir::new().expect("ATM runtime temp");
    let output = Command::new(executable)
        .args(args)
        .env("ATM_HOME", temp.path())
        .env("ATM_CONFIG_HOME", temp.path().join("config"))
        .env("ATM_LOG_DIR", temp.path().join("logs"))
        .env("ATM_TEAMS_DIR", temp.path().join("teams"))
        .output()
        .expect("run ATM help");
    assert!(
        output.status.success(),
        "ATM help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("ATM help output is UTF-8")
}

#[test]
fn live_cron_help_covers_paths_flags_and_has_real_explanations() {
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_atm"));
    let cases: [(&[&str], &[&str]); 4] = [
        (
            &["cron", "--help"],
            &[
                "Manage persistent UTC schedules",
                "exactly one stop condition",
                "start",
                "end",
                "list",
            ],
        ),
        (
            &["cron", "start", "--help"],
            &[
                "--cron",
                "--module",
                "--phase",
                "--until",
                "--until-hook",
                "--arg",
                "--cwd",
                "--custom",
                "seconds first",
                "future occurrence",
            ],
        ),
        (
            &["cron", "end", "--help"],
            &["--phase", "[NAME]", "every active schedule", "manual stop"],
        ),
        (
            &["cron", "list", "--help"],
            &["--phase", "--json", "bare array", "last_outcome"],
        ),
    ];
    for (args, expected) in cases {
        let output = run_help(&executable, args);
        for needle in expected {
            assert!(
                output
                    .to_ascii_lowercase()
                    .contains(&needle.to_ascii_lowercase()),
                "help for {args:?} omitted {needle:?}: {output}"
            );
        }
    }
}

#[cfg(unix)]
fn symlink_file(source: &Path, target: &Path) {
    std::os::unix::fs::symlink(source, target).expect("create symlink");
}

#[cfg(windows)]
fn symlink_file(source: &Path, target: &Path) {
    std::os::windows::fs::symlink_file(source, target).expect("create symlink");
}

#[test]
fn staged_help_resolves_cron_doc_from_binary_and_symlink() {
    let root = repo_root();
    let temp = TempDir::new().expect("staging root");
    let install = temp.path().join("install");
    let bin_dir = install.join("bin");
    let docs_dir = install.join("share/doc/atm");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    copy_tree(&root.join("docs/user-documents"), &docs_dir);
    let installed_atm = bin_dir.join(if cfg!(windows) { "atm.exe" } else { "atm" });
    fs::copy(env!("CARGO_BIN_EXE_atm"), &installed_atm).expect("copy atm binary");

    let result: Value =
        serde_json::from_str(&run_help(&installed_atm, &["help", "cron", "--json"]))
            .expect("installed help JSON");
    let expected_doc = docs_dir.join("cron.md").canonicalize().unwrap();
    assert_eq!(
        PathBuf::from(result["installed_doc_topic_path"].as_str().unwrap()),
        expected_doc
    );
    assert!(Path::new(result["installed_doc_topic_path"].as_str().unwrap()).is_file());

    let shim_dir = temp.path().join("shim");
    fs::create_dir_all(&shim_dir).expect("shim dir");
    let shim = shim_dir.join(if cfg!(windows) { "atm.exe" } else { "atm" });
    symlink_file(&installed_atm, &shim);
    let through_symlink: Value =
        serde_json::from_str(&run_help(&shim, &["help", "cron", "--json"]))
            .expect("symlink help JSON");
    assert_eq!(
        through_symlink["installed_doc_topic_path"],
        result["installed_doc_topic_path"]
    );
}

#[test]
fn release_archives_include_cron_guide_on_tar_and_zip_targets() {
    let root = repo_root();
    let temp = TempDir::new().expect("archive staging root");
    let manifest = root.join("release/publish-artifacts.toml");
    let verifier = root.join("scripts/verify_release_archive.py");
    let python = r#"
import pathlib, sys, tarfile, zipfile
root, archive, kind = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
if kind == "zip":
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
        for path in root.rglob("*"):
            if path.is_file():
                output.write(path, path.relative_to(root).as_posix())
else:
    with tarfile.open(archive, "w:gz") as output:
        for path in root.rglob("*"):
            if path.is_file():
                output.add(path, path.relative_to(root).as_posix())
"#;
    for (target, kind, binary_suffix) in [
        ("linux", "tar", ""),
        ("macos", "tar", ""),
        ("windows", "zip", ".exe"),
    ] {
        let stage = temp.path().join(target);
        let bin = stage.join("bin");
        fs::create_dir_all(&bin).expect("staged bin");
        for name in ["atm", "atm-daemon"] {
            fs::write(bin.join(format!("{name}{binary_suffix}")), "fixture binary")
                .expect("staged binary");
        }
        copy_tree(
            &root.join("docs/user-documents"),
            &stage.join("share/doc/atm"),
        );
        let archive = temp.path().join(format!(
            "atm-{target}.{}",
            if kind == "zip" { "zip" } else { "tar.gz" }
        ));
        let generated = Command::new("python3")
            .arg("-c")
            .arg(python)
            .arg(&stage)
            .arg(&archive)
            .arg(kind)
            .status()
            .expect("run deterministic archive builder");
        assert!(
            generated.success(),
            "archive generation failed for {target}"
        );
        let checked = Command::new("python3")
            .arg(&verifier)
            .arg("--manifest")
            .arg(&manifest)
            .arg("--archive")
            .arg(&archive)
            .output()
            .expect("run release archive verifier");
        assert!(
            checked.status.success(),
            "archive verifier failed for {target}: {}",
            String::from_utf8_lossy(&checked.stderr)
        );
        assert!(
            String::from_utf8_lossy(&checked.stdout).contains("share/doc/atm/cron.md"),
            "archive proof omitted cron.md for {target}"
        );
    }
}
