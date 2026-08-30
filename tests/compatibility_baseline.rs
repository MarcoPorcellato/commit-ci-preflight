use std::process::{Command, Output};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const EVALUATED_AT: &str = "2026-08-08T12:30:00Z";

fn ccp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(args)
        .output()
        .expect("execute compatibility command")
}

#[test]
fn root_help_bytes_match_the_baseline() {
    let output = ccp(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        include_bytes!("fixtures/compatibility/root-help.stdout.txt")
    );
}

#[test]
fn command_help_bytes_match_the_baseline() {
    for (command, expected) in [
        (
            "plan",
            include_bytes!("fixtures/compatibility/plan-help.stdout.txt").as_slice(),
        ),
        (
            "verify",
            include_bytes!("fixtures/compatibility/verify-help.stdout.txt").as_slice(),
        ),
    ] {
        let o = ccp(&[command, "--help"]);
        assert_eq!(o.status.code(), Some(0));
        assert!(o.stderr.is_empty());
        assert_eq!(o.stdout, expected, "{command} help drifted");
    }
}

#[test]
fn plan_and_verification_bytes_and_exit_codes_match_the_baseline() {
    let p = ccp(&[
        "plan",
        "--config",
        "tests/fixtures/config-v1-read-only.toml",
        "--json",
    ]);
    assert_eq!(p.status.code(), Some(0));
    assert!(p.stderr.is_empty());
    assert_eq!(
        p.stdout,
        include_bytes!("fixtures/compatibility/plan-v1.stdout.json")
    );
    let a = [
        "verify",
        "--receipt",
        "tests/fixtures/receipt-v1-pass.json",
        "--policy",
        "tests/fixtures/policy-v1.toml",
        "--expected-commit",
        COMMIT,
        "--evaluated-at-utc",
        EVALUATED_AT,
        "--json",
    ];
    let v = ccp(&a);
    assert_eq!(v.status.code(), Some(0));
    assert!(v.stderr.is_empty());
    assert_eq!(
        v.stdout,
        include_bytes!("fixtures/compatibility/verify-v1-pass.stdout.json")
    );
    let f = ccp(&[
        "verify",
        "--receipt",
        "tests/fixtures/receipt-v1-pass.json",
        "--policy",
        "tests/fixtures/policy-v1.toml",
        "--expected-commit",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "--evaluated-at-utc",
        EVALUATED_AT,
        "--json",
    ]);
    assert_eq!(f.status.code(), Some(3));
    assert_eq!(
        f.stdout,
        include_bytes!("fixtures/compatibility/verify-v1-fail.stdout.json")
    );
}

#[test]
fn matrix_plan_profiles_match_the_baseline() {
    let c = ccp(&[
        "plan",
        "--config",
        "tests/fixtures/config-v2-matrix.toml",
        "--json",
    ]);
    assert_eq!(c.status.code(), Some(0));
    assert!(c.stderr.is_empty());
    assert_eq!(
        c.stdout,
        include_bytes!("fixtures/plan-v2-current-default.stdout.json")
    );
    let l = ccp(&[
        "plan",
        "--config",
        "tests/fixtures/config-v2-legacy-compatible.toml",
        "--matrix-plan-profile",
        "matrix-v2-legacy-v1",
        "--json",
    ]);
    assert_eq!(l.status.code(), Some(0));
    assert!(l.stderr.is_empty());
    assert_eq!(
        l.stdout,
        include_bytes!("fixtures/compatibility/plan-v2-legacy.stdout.json")
    );
}

fn normalize_mount_argv(
    argument: &str,
    sources: &std::collections::BTreeMap<String, String>,
) -> String {
    let Some(rest) = argument.strip_prefix("type=bind,src=") else {
        return argument.to_owned();
    };
    let (source, suffix) = rest.split_once(",dst=").expect("bind destination");
    let token = sources.get(source).expect("declared mount source");
    format!("type=bind,src={token},dst={suffix}")
}

fn normalize_dry_run(mut v: serde_json::Value) -> (serde_json::Value, Vec<String>) {
    let mut paths = Vec::new();
    fn collect_one(value: &serde_json::Value, paths: &mut Vec<String>) {
        let workspace = &value["workspace"];
        paths.push(workspace["repository"].as_str().unwrap().to_owned());
        paths.push(workspace["run_root"].as_str().unwrap().to_owned());
        for mount in workspace["mounts"].as_array().unwrap() {
            paths.push(mount["source"].as_str().unwrap().to_owned());
        }
    }
    if let Some(runtimes) = v.get("runtimes").and_then(|x| x.as_array()) {
        for runtime in runtimes {
            collect_one(&runtime["dry_run"], &mut paths);
        }
    } else {
        collect_one(&v, &mut paths);
    }
    fn one(v: &mut serde_json::Value) {
        v["workspace"]["repository"] = serde_json::json!("$REPOSITORY");
        v["workspace"]["run_root"] = serde_json::json!("$RUN_ROOT");
        let mut m = std::collections::BTreeMap::new();
        m.insert("$REPOSITORY".to_string(), "$REPOSITORY".to_string());
        for x in v["workspace"]["mounts"].as_array_mut().unwrap() {
            let s = x["source"].as_str().unwrap().to_string();
            let t = match (
                x["purpose"].as_str().unwrap(),
                x.get("logical_id").and_then(|x| x.as_str()),
            ) {
                ("repository", _) => "$REPOSITORY".into(),
                ("cache", Some(i)) => format!("$CACHE:{i}"),
                ("artifact", Some(i)) => format!("$ARTIFACT:{i}"),
                _ => panic!(),
            };
            m.insert(s, t.clone());
            x["source"] = serde_json::json!(t);
        }
        for c in v["checks"].as_array_mut().unwrap() {
            for a in c["argv"].as_array_mut().unwrap() {
                let s = a.as_str().unwrap();
                if let Some(r) = s.strip_prefix("type=bind,src=") {
                    *a = serde_json::json!(normalize_mount_argv(s, &m));
                }
            }
        }
    }
    if let Some(rs) = v.get_mut("runtimes").and_then(|x| x.as_array_mut()) {
        for r in rs {
            one(&mut r["dry_run"]);
        }
    } else {
        one(&mut v)
    }
    (v, paths)
}

#[test]
fn bind_source_lookup_requires_exact_path_equality() {
    let mut sources = std::collections::BTreeMap::new();
    sources.insert("/cache/cargo".to_owned(), "$CACHE:cargo".to_owned());
    sources.insert("/cache/cargo-extra".to_owned(), "$CACHE:extra".to_owned());
    assert_eq!(
        normalize_mount_argv(
            "type=bind,src=/cache/cargo-extra,dst=/workspace/cache",
            &sources
        ),
        "type=bind,src=$CACHE:extra,dst=/workspace/cache"
    );
}

fn object_keys(value: &serde_json::Value) -> std::collections::BTreeSet<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_one_dry_run_contract(original: &serde_json::Value, normalized: &serde_json::Value) {
    assert_eq!(object_keys(original), object_keys(normalized));
    for field in [
        "schema_version",
        "plan_digest",
        "runtime",
        "program",
        "workspace_mount_policy",
        "executed",
    ] {
        assert_eq!(original[field], normalized[field], "{field}");
    }
    assert_eq!(original["executed"], false);
    let before = original["checks"].as_array().expect("checks");
    let after = normalized["checks"].as_array().expect("checks");
    assert_eq!(before.len(), after.len());
    for (before, after) in before.iter().zip(after) {
        assert_eq!(object_keys(before), object_keys(after));
        for field in ["id", "program", "depends_on"] {
            assert_eq!(before[field], after[field], "{field}");
        }
        let ba = before["argv"].as_array().unwrap();
        let aa = after["argv"].as_array().unwrap();
        assert_eq!(ba.len(), aa.len());
        for (raw, scrubbed) in ba.iter().zip(aa) {
            let raw = raw.as_str().unwrap();
            let scrubbed = scrubbed.as_str().unwrap();
            if let Some(raw_mount) = raw.strip_prefix("type=bind,src=") {
                let (_, suffix) = raw_mount.split_once(",dst=").unwrap();
                let normalized_mount = scrubbed.strip_prefix("type=bind,src=$").unwrap();
                let (_, normalized_suffix) = normalized_mount.split_once(",dst=").unwrap();
                assert_eq!(suffix, normalized_suffix);
            } else {
                assert_eq!(raw, scrubbed);
            }
        }
    }
    let bw = &original["workspace"];
    let aw = &normalized["workspace"];
    assert_eq!(object_keys(bw), object_keys(aw));
    assert_eq!(bw["schema_version"], aw["schema_version"]);
    assert_eq!(bw["source_snapshot_digest"], aw["source_snapshot_digest"]);
    let bm = bw["mounts"].as_array().unwrap();
    let am = aw["mounts"].as_array().unwrap();
    assert_eq!(bm.len(), am.len());
    for (before, after) in bm.iter().zip(am) {
        assert_eq!(object_keys(before), object_keys(after));
        for field in ["target", "access", "purpose", "logical_id"] {
            assert_eq!(before.get(field), after.get(field), "{field}");
        }
    }
}

fn assert_dry_run_normalization_preserves_contract(
    original: &serde_json::Value,
    normalized: &serde_json::Value,
) {
    assert_eq!(object_keys(original), object_keys(normalized));
    match (original.get("runtimes"), normalized.get("runtimes")) {
        (Some(before), Some(after)) => {
            let before = before.as_array().unwrap();
            let after = after.as_array().unwrap();
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(after) {
                assert_eq!(object_keys(before), object_keys(after));
                assert_eq!(before["runtime_id"], after["runtime_id"]);
                assert_eq!(
                    before["configuration_digest"],
                    after["configuration_digest"]
                );
                assert_one_dry_run_contract(&before["dry_run"], &after["dry_run"]);
            }
        }
        (None, None) => assert_one_dry_run_contract(original, normalized),
        _ => panic!("matrix shape changed"),
    }
}

#[test]
fn dry_run_profiles_match_the_baseline_without_execution() {
    for (a, e) in [
        (
            vec![
                "dry-run",
                "--config",
                "tests/fixtures/config-v1-read-only.toml",
                "--json",
            ],
            include_bytes!("fixtures/compatibility/dry-run-v1.normalized.json").as_slice(),
        ),
        (
            vec![
                "dry-run",
                "--config",
                "tests/fixtures/config-v2-matrix.toml",
                "--json",
            ],
            include_bytes!("fixtures/compatibility/dry-run-v2-current.normalized.json").as_slice(),
        ),
        (
            vec![
                "dry-run",
                "--config",
                "tests/fixtures/config-v2-legacy-compatible.toml",
                "--matrix-plan-profile",
                "matrix-v2-legacy-v1",
                "--json",
            ],
            include_bytes!("fixtures/compatibility/dry-run-v2-legacy.normalized.json").as_slice(),
        ),
    ] as [(Vec<&str>, &[u8]); 3]
    {
        let o = ccp(&a);
        assert_eq!(o.status.code(), Some(0));
        assert!(o.stderr.is_empty());
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        let original = v.clone();
        let (n, host_paths) = normalize_dry_run(v);
        assert_dry_run_normalization_preserves_contract(&original, &n);
        let serialized = serde_json::to_vec(&n).unwrap();
        for path in host_paths {
            assert!(!serialized.windows(path.len()).any(|w| w == path.as_bytes()));
        }
        assert_eq!(serialized, e.strip_suffix(b"\n").unwrap_or(e));
    }
}

#[test]
fn usage_error_exit_code_remains_two() {
    let o = ccp(&["plan", "--config", "tests/fixtures/does-not-exist.toml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
}

#[test]
#[ignore]
fn print_normalized_dry_run_baselines() {
    for a in [
        vec![
            "dry-run",
            "--config",
            "tests/fixtures/config-v1-read-only.toml",
            "--json",
        ],
        vec![
            "dry-run",
            "--config",
            "tests/fixtures/config-v2-matrix.toml",
            "--json",
        ],
        vec![
            "dry-run",
            "--config",
            "tests/fixtures/config-v2-legacy-compatible.toml",
            "--matrix-plan-profile",
            "matrix-v2-legacy-v1",
            "--json",
        ],
    ] {
        let o = ccp(&a);
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        println!(
            "{}",
            serde_json::to_string(&normalize_dry_run(v).0).unwrap()
        );
    }
}
