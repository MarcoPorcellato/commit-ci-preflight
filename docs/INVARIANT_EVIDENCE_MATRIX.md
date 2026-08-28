# T2 invariant evidence matrix

## Status

This matrix records the implemented T2 snapshot tranche. It separates
deterministic source evidence from native qualification still pending.

## Reading guide

- **Implemented evidence**: deterministic source evidence present in this tranche.
- **Residual gap**: evidence that still requires native or fault qualification.
- **Proof artifact**: the durable implementation or test evidence.
- **Gate**: the measurable acceptance condition and its current status.

## Matrix

| Invariant | Implemented evidence | Residual gap | Proof artifact | Gate |
| --- | --- | --- | --- | --- |
| Exact commit bytes are isolated from the user's mutable working tree | `SourceSnapshot::materialize` reads the committed tree and blobs through Git, writes a CCP-owned tree, and `PreparedWorkspace::prepare_snapshot` mounts only that tree; source blobs use a dedicated 64 MiB ceiling while ordinary process output remains limited to 1 MiB | Native runtime observation remains pending | `src/source_snapshot.rs`, `src/process.rs`, and the real-Git large-blob snapshot test | Deterministic isolation PASS; native qualification PENDING |
| Source identity is canonical and reproducible | `SourceManifestV1` sorts entries and binds commit, path, mode, object kind and object ID; canonical SHA-256 produces `manifest_digest` | Cross-platform native vectors remain pending | Source-snapshot unit tests and receipt v2 golden fixture | Repeated supported manifests have identical digest: PASS |
| Unsupported Git states fail closed | Submodules, symlinks and LFS pointers are rejected; unsupported modes fail; executable entries are supported on Unix and rejected on unsupported platforms. Sparse working-tree shape is ignored because materialization reads the full committed tree; unavailable objects fail as Git errors | Windows-native executable-mode behavior is not qualified | Typed `SourceSnapshotError` variants and deterministic rejection tests | Deterministic policy PASS; Windows-native qualification PENDING |
| Receipt evidence binds source identity | Snapshot-backed runs publish strict receipt v2 with strategy, manifest digest and entry count; historical v1 remains readable without implied snapshot assurance | Trusted producer identity and signing are later tranches | `schema/receipt-v2.schema.json`, `tests/fixtures/receipt-v2-pass.json`, and dual-version verifier dispatch | Tampered snapshot digest fails integrity: PASS |
| Source identity is revalidated before sealing | `SourceSnapshot::revalidate` checks file set, mode, blob identity and manifest digest after execution and before receipt construction | Native interruption evidence remains pending | Run lifecycle ordering and source revalidation tests | Changed snapshot cannot produce a sealed receipt: PASS |
| Snapshot cleanup is bounded and recoverable | The journal reserves one opaque resource, records `source-snapshot-v1.json` with strict source identity, and existing run quarantine owns the complete run directory; cleanup targets only the exact snapshot resource | Crash/power-loss and Windows replacement receipts remain pending | `RunJournalSourceV1`, create-new durable binding, recovery validation and fault-injection seam | Deterministic ownership PASS; native crash qualification PENDING |
| Qualification remains truthful | Deterministic tests cover materialization, revalidation, journal binding, v2 publication, v1/v2 verification and tamper rejection | Native platform and crash/power-loss receipts are pending | Separate source-test report and future native receipts | No unavailable native run is called PASS |

## Current boundary

The implemented source boundary is explicit:

- the mutable repository is used to resolve a clean exact `HEAD`, not as the
  attestable runtime mount;
- the runtime receives only the CCP-owned source snapshot;
- writable state is limited to declared cache and artifact bindings;
- the runtime renders explicit argv instead of using an implicit shell;
- receipt v2 carries source identity while receipt v1 remains a strict legacy
  contract;
- the run journal owns source cleanup without recording host paths.

This closes T2 in deterministic source evidence. Native platform,
crash/power-loss and release qualification remain separate gates.
## Task 8 documentation evidence matrix

| Invariant | Focused evidence |
|---|---|
| Projection reproducibility | `tests/matrix_contract.rs::legacy_profile_reproduces_historical_plan` |
| Representability rejection | `tests/matrix_contract.rs::legacy_profile_rejects_each_non_representable_current_field` |
| Command parity | `tests/plan_cli.rs::matrix_plan_profile_flag_is_exposed_only_by_configuration_commands` |
| Cache separation | `tests/runtime_cli.rs::legacy_profile_uses_distinct_plan_cache_identity` |
| Producer uniformity | `tests/matrix_contract.rs::legacy_receipt_provenance_is_uniform` |
| Historical verifier acceptance | `tests/verification_contract.rs::historical_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations` (`#[ignore]`, `--ignored`, external verifier required) |
| Mutation rejection | `tests/verification_contract.rs::current_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations` and historical verifier test |
| Zero pre-admission mutation | `tests/runtime_cli.rs::legacy_profile_rejection_precedes_shared_state` and `tests/runtime_cli.rs::legacy_profile_rejects_current_only_matrix_syntax_before_shared_state` |

All entries are Matrix-only evidence for `matrix-v2-legacy-v1`; they do not infer
policy or establish general trust. Rollback target is `current-v2`.
