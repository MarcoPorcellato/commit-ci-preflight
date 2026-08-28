use std::fs;
use std::path::Path;

#[test]
fn cache_pin_documentation_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = [
        "docs/CACHE_AND_WORKSPACE.md",
        "docs/RUNTIME.md",
        "docs/LOCAL_RUN.md",
        "docs/COORDINATION_RUNBOOK.md",
        "docs/TESTING_AND_FAULT_INJECTION.md",
        "docs/THREAT_MODEL.md",
    ]
    .map(|path| {
        (
            path,
            fs::read_to_string(root.join(path)).expect("read contract doc"),
        )
    });
    let combined_docs = docs
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for required in [
        "--managed-cache-root",
        "--managed-cache-source",
        "spawn-boundary revalidation",
        "undeclared paths are not pinned",
        "manual deletion remains unsupported",
    ] {
        assert!(combined_docs.contains(required), "missing {required}");
    }

    for forbidden in [
        "cache pin uses a TTL lease",
        "automatic cache deletion is enabled",
        "guard exec emits a receipt",
        "external same-path replacement is prevented",
    ] {
        assert!(
            !combined_docs.contains(forbidden),
            "forbidden claim: {forbidden}"
        );
    }

    for (path, required) in [
        ("docs/CACHE_AND_WORKSPACE.md", "--managed-cache-source"),
        ("docs/RUNTIME.md", "spawn-boundary revalidation"),
        ("docs/LOCAL_RUN.md", "--managed-cache-root"),
        (
            "docs/COORDINATION_RUNBOOK.md",
            "undeclared paths are not pinned",
        ),
        ("docs/TESTING_AND_FAULT_INJECTION.md", "non-cooperative"),
        (
            "docs/THREAT_MODEL.md",
            "manual deletion remains unsupported",
        ),
    ] {
        let text = docs
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, text)| text)
            .expect("named contract document");
        assert!(text.contains(required), "{path} missing {required}");
    }
}

#[test]
fn local_run_documentation_does_not_present_dry_run_argv_as_a_replay_bundle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let local_run =
        fs::read_to_string(root.join("docs/LOCAL_RUN.md")).expect("read local run contract");
    let normalized = local_run.split_whitespace().collect::<Vec<_>>().join(" ");

    for required in [
        "planning and mount-review surface",
        "does not create cache directories",
        "entries/sha256-<key>/data",
        "private per-generation staging sources",
        "must be independently proven to exist",
        "owns its writable lifecycle",
    ] {
        assert!(normalized.contains(required), "missing {required}");
    }

    assert!(
        !normalized.contains("reproduce a failing command deliberately"),
        "dry-run argv must not be presented as a self-contained replay bundle"
    );
}
