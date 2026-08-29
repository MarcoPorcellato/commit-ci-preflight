use std::process::Command;

#[test]
fn help_exposes_only_verification_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_ccp-verifier"))
        .arg("--help")
        .output()
        .expect("verifier help");
    assert_eq!(output.status.code(), Some(0));
    let help = String::from_utf8(output.stdout).expect("help UTF-8");
    assert!(help.contains("verify"));
    assert!(help.contains("schema"));
    for forbidden in [
        "run",
        "plan",
        "doctor",
        "dry-run",
        "benchmark",
        "guard",
        "migrate",
    ] {
        assert!(!help.contains(forbidden), "unexpected command: {forbidden}");
    }
}

#[test]
fn verify_help_documents_compatibility_defaults() {
    let output = Command::new(env!("CARGO_BIN_EXE_ccp-verifier"))
        .args(["verify", "--help"])
        .output()
        .expect("verify help");
    let help = String::from_utf8(output.stdout).expect("help UTF-8");
    assert!(help.contains(".ccp/receipt.json"));
    assert!(help.contains(".commit-ci-policy.toml"));
}

#[test]
fn verify_requires_explicit_commit_and_reports_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_ccp-verifier"))
        .args([
            "verify",
            "--receipt",
            "receipt.json",
            "--policy",
            "policy.toml",
        ])
        .output()
        .expect("verify CLI");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
