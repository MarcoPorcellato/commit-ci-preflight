use std::process::Command;

fn run(kind: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ccp-verifier"))
        .args(["schema", "--kind", kind])
        .output()
        .expect("schema CLI")
}

#[test]
fn schema_kinds_emit_checked_in_bytes() {
    for (kind, path) in [
        ("receipt-v1", "../../schema/receipt-v1.schema.json"),
        ("receipt-v2", "../../schema/receipt-v2.schema.json"),
        ("policy-v1", "../../schema/policy-v1.schema.json"),
        ("policy-v1-1", "../../schema/policy-v1_1.schema.json"),
        ("policy-v2", "../../schema/policy-v2.schema.json"),
        (
            "verification-report-v1",
            "../../schema/verification-report-v1.schema.json",
        ),
    ] {
        let output = run(kind);
        assert_eq!(output.status.code(), Some(0), "{kind}");
        let expected = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path.strip_prefix("../../").unwrap_or(path));
        assert_eq!(
            output.stdout,
            std::fs::read(expected).expect("schema fixture")
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn unknown_schema_kind_is_usage_error() {
    let output = run("unknown");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
