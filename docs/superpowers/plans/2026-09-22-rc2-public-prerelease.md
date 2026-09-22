# v0.1.0-rc.2 Public Prerelease Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish a reviewable macOS arm64 `v0.1.0-rc.2` prerelease candidate
with a reproducible release bundle and honest, test-backed cost and
time-to-feedback reporting.

**Architecture:** Keep the product CLI unchanged. Add a small typed library
module plus an offline example that evaluates explicitly supplied comparable
event data; fixtures prove the arithmetic and rejection behavior. Keep public
case-study prose in documentation, and make the release script generate one
labelled archive with an in-archive provenance manifest and checksum.

**Tech Stack:** Rust 2024, existing `serde` and `serde_json`, Bash, Cargo,
GitHub-hosted CI, Markdown, SPDX 2.3.

**Spec:**
[`docs/superpowers/specs/2026-09-22-rc2-public-prerelease-design.md`](../specs/2026-09-22-rc2-public-prerelease-design.md)

## Global Constraints

- All public-facing prose, release notes, errors, manifests, and examples are
  English.
- `v0.1.0-rc.2` is a GitHub prerelease, never a stable `v0.1.0` claim.
- Publish only an unsigned macOS arm64 archive, its SHA-256 manifest, source
  tag/archive, SPDX SBOM, notices, English installation/rollback guidance, and
  support/economic documentation.
- Do not add registry packages, installers, signing, keys, KMS, Homebrew,
  Windows/Linux binaries, automatic upgrades, or a stable-support claim.
- Do not change CLI, receipt, policy, configuration, or schema behavior.
- Standard GitHub-hosted CI on public repositories is not a monetary-saving
  use case. Keep ordinary public pull requests on hosted CI.
- Public claims distinguish avoided remote compute, preserved included quota,
  GitHub charges avoided, and uncertified net savings.
- Time-to-feedback is distinct from money. A speed claim requires comparable
  measured events; it must disclose local preparation overhead and never claim
  a universal speed-up.
- Release commands and test commands in this plan begin with `rtk`; no command
  may tag, push, upload, publish, sign, or replace an installed binary.

## Review Focus

1. A public-standard-runner input with a zero rate must render zero avoided
   GitHub charges, not a fictional monetary benefit; Task 2 owns this test.
2. An event whose hosted and local test-scope identifiers differ must be
   rejected before any median or saving is calculated; Task 2 owns this test.
3. Cancelled, failed, or incomplete events must be counted and reported but
   cannot silently enter the comparable successful-event sample; Task 2 owns
   this test.
4. Inverted or missing timestamps must fail closed rather than produce a
   negative queue delay or misleading feedback duration; Task 2 owns this
   test.
5. A prerelease archive must disclose its exact source commit and release
   label while leaving the GitHub-generated source archive outside the
   maintainer-asset checksum set; Tasks 4 and 6 own its static and assembled
   checks.

---

## File Structure

| Path | Responsibility |
|---|---|
| `src/economic_evaluation.rs` | Typed, bounded parsing and deterministic aggregation of one operator-supplied comparison worksheet. |
| `src/lib.rs` | Exposes the evaluation module to the offline example and integration tests without changing the CCP CLI. |
| `examples/evaluate_economic_case_study.rs` | Reads one worksheet JSON file and prints one privacy-preserving JSON report. |
| `tests/economic_evaluation_contract.rs` | TDD contract for timestamps, scope equivalence, rounding, classifications, and claim boundaries. |
| `tests/fixtures/economic-evaluation-v1/*.json` | Synthetic valid and invalid worksheets; never marketing evidence. |
| `docs/TIME_TO_FEEDBACK_EVALUATION.md` | English method, data dictionary, reporting template, and non-claim boundary. |
| `docs/ECONOMIC_QUALIFICATION.md` | Links cost cases to the time-evaluation method and preserves current measured/counterfactual labels. |
| `README.md` | Concise English audience-facing explanation of cost and feedback-time claims. |
| `scripts/build_release_candidate.sh` | Builds one labelled, non-publishing candidate archive and its manifest/checksum. |
| `tests/release_hardening_contract.rs` | Pins archive naming, provenance-manifest content, included English documentation, and non-publishing boundaries. |
| `docs/INSTALLATION.md` | Verifies the RC.2 archive and explains the maintainer-asset versus GitHub-source-archive boundary. |
| `docs/UPGRADE_AND_ROLLBACK.md` | Keeps an explicit candidate-to-rollback procedure with no identity overclaim. |
| `docs/BETA_SUPPORT.md` | Updates RC.2 surface status without relabelling pending platform work as qualified. |
| `CHANGELOG.md` | Adds concise RC.2 unreleased entries for release reproducibility and evidence reporting. |

### Task 1: Define the worksheet and report boundary

**Files:**
- Create: `src/economic_evaluation.rs`
- Modify: `src/lib.rs`
- Create: `tests/economic_evaluation_contract.rs`
- Create: `tests/fixtures/economic-evaluation-v1/valid-ten-events.json`

**Interfaces:**
- Consumes: one UTF-8 JSON worksheet whose `schema_version` is exactly
  `economic-evaluation-v1`.
- Produces: `economic_evaluation::EvaluationReport` through
  `evaluate_worksheet(&EvaluationWorksheet) -> Result<EvaluationReport, EvaluationError>`.
- Later tasks rely on: the stable JSON report fields listed below and the fact
  that the module never reads GitHub, local billing, environment, or receipts.

- [ ] **Step 1: Write the failing integration test for a ten-event worksheet**

  Add the following imports and assertions to
  `tests/economic_evaluation_contract.rs`:

  ```rust
  use commit_ci_preflight::economic_evaluation::{
      evaluate_worksheet, EvaluationClass, EvaluationWorksheet,
  };

  #[test]
  fn ten_equivalent_successes_produce_measured_time_and_cost_fields() {
      let worksheet: EvaluationWorksheet = serde_json::from_str(include_str!(
          "fixtures/economic-evaluation-v1/valid-ten-events.json"
      ))
      .expect("valid fixture");

      let report = evaluate_worksheet(&worksheet).expect("evaluate fixture");

      assert_eq!(report.classification, EvaluationClass::Measured);
      assert_eq!(report.comparable_success_count, 10);
      assert_eq!(report.excluded_event_count, 0);
      assert_eq!(report.hosted_median_end_to_end_seconds, 180);
      assert_eq!(report.local_median_end_to_end_seconds, 72);
      assert_eq!(report.avoided_rounded_hosted_minutes, 20);
      assert_eq!(report.avoided_github_charge_microusd, 120_000);
  }
  ```

- [ ] **Step 2: Run the focused test and confirm it fails because the module is absent**

  Run:

  ```console
  rtk cargo test --locked --test economic_evaluation_contract ten_equivalent_successes_produce_measured_time_and_cost_fields
  ```

  Expected: compilation fails because `economic_evaluation` does not exist.

- [ ] **Step 3: Add the minimal typed input and output model**

  In `src/economic_evaluation.rs`, define these public types with
  `Deserialize`/`Serialize` where needed:

  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
  pub struct EvaluationWorksheet {
      pub schema_version: String,
      pub runner_rate_microusd_per_minute: u64,
      pub events: Vec<EvaluationEvent>,
  }

  #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
  pub struct EvaluationEvent {
      pub outcome: EventOutcome,
      pub hosted_scope_id: String,
      pub local_scope_id: String,
      pub hosted_dispatched_at_seconds: Option<u64>,
      pub hosted_runner_started_at_seconds: Option<u64>,
      pub hosted_completed_at_seconds: Option<u64>,
      pub hosted_job_seconds: Vec<u64>,
      pub retained_hosted_job_seconds: Vec<u64>,
      pub local_started_at_seconds: Option<u64>,
      pub local_completed_at_seconds: Option<u64>,
      pub local_verified_at_seconds: Option<u64>,
      pub local_preparation_seconds: Option<u64>,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum EventOutcome { Success, Failure, Cancelled, Incomplete }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum EvaluationClass { Measured, Exploratory }

  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
  pub struct OutcomeCounts {
      pub success: u64,
      pub failure: u64,
      pub cancelled: u64,
      pub incomplete: u64,
  }

  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct EvaluationError(pub String);
  ```

  Add `EvaluationReport` with the exact fields asserted above plus
  `hosted_median_queue_seconds`, `local_median_execution_seconds`,
  `local_median_preparation_seconds`, `hosted_range_end_to_end_seconds`,
  `local_range_end_to_end_seconds`, and an outcome-count map. Use integer
  seconds and integer micro-USD only; do not add a floating-point or currency
  dependency. Implement `Display` and `std::error::Error` for
  `EvaluationError` in this task.

  In `src/lib.rs`, add exactly:

  ```rust
  pub mod economic_evaluation;
  ```

- [ ] **Step 4: Implement the smallest valid aggregation**

  Implement `evaluate_worksheet` so it:

  ```rust
  const SCHEMA_VERSION: &str = "economic-evaluation-v1";
  const MEASURED_MINIMUM: usize = 10;

  fn rounded_minutes(seconds: u64) -> u64 {
      seconds.div_ceil(60)
  }
  ```

  - rejects any other schema version, an empty event set, an empty scope ID,
    mismatched scope IDs for a successful event, a successful event without
    all timestamps, an empty or zero-length hosted/retained job, or
    non-monotonic timestamps;
  - keeps failure, cancellation, and incomplete outcomes in `outcome_counts`
    and increments `excluded_event_count` without using them for medians;
  - calculates hosted queue as runner start minus dispatch, hosted end-to-end
    as completion minus dispatch, local execution as completion minus local
    start, and local end-to-end as verification minus local start;
  - calculates avoided rounded minutes per event as
    `sum(ceil(hosted_job_seconds / 60)) - sum(ceil(retained_hosted_job_seconds / 60))`,
    rejecting a zero or negative result instead of reporting a saving; and
  - returns `Measured` for ten or more comparable successes, otherwise
    `Exploratory`; both results retain the raw comparable count.

  The valid fixture must contain ten synthetic success events with identical
  opaque scope IDs per event, two 61-second hosted jobs and two retained
  60-second gate jobs, hosted end-to-end durations whose median is 180 seconds,
  and local verified end-to-end durations whose median is 72 seconds. Set the
  rate to `6_000` micro-USD per minute, so twenty avoided rounded minutes
  produce `120_000` micro-USD.

- [ ] **Step 5: Run the focused test and commit the working slice**

  Run:

  ```console
  rtk cargo test --locked --test economic_evaluation_contract ten_equivalent_successes_produce_measured_time_and_cost_fields
  ```

  Expected: PASS.

  Commit:

  ```console
  rtk git add src/lib.rs src/economic_evaluation.rs tests/economic_evaluation_contract.rs tests/fixtures/economic-evaluation-v1/valid-ten-events.json
  rtk git commit -m "feat: add bounded economic evaluation model"
  ```

### Task 2: Fail closed and preserve non-success observations

**Files:**
- Modify: `src/economic_evaluation.rs`
- Modify: `tests/economic_evaluation_contract.rs`
- Create: `tests/fixtures/economic-evaluation-v1/invalid-scope-mismatch.json`
- Create: `tests/fixtures/economic-evaluation-v1/invalid-timestamp-order.json`
- Create: `tests/fixtures/economic-evaluation-v1/nine-successes-one-cancelled.json`
- Create: `tests/fixtures/economic-evaluation-v1/zero-rate-public-runner.json`

**Interfaces:**
- Consumes: the Task 1 worksheet and report API.
- Produces: stable `EvaluationError` messages and outcome-aware report fields.
- Later tasks rely on: a zero billed rate reporting zero charge without being
  presented as an economic qualification.

- [ ] **Step 1: Write failing tests for boundary inputs**

  Add these tests:

  ```rust
  #[test]
  fn mismatched_test_scopes_fail_before_aggregation() {
      let worksheet = fixture("invalid-scope-mismatch.json");
      assert!(evaluate_worksheet(&worksheet)
          .expect_err("scope mismatch must fail")
          .to_string()
          .contains("hosted_scope_id must equal local_scope_id"));
  }

  #[test]
  fn non_monotonic_timestamps_fail_before_aggregation() {
      let worksheet = fixture("invalid-timestamp-order.json");
      assert!(evaluate_worksheet(&worksheet)
          .expect_err("time order must fail")
          .to_string()
          .contains("timestamps must be monotonic"));
  }

  #[test]
  fn nine_successes_and_one_cancelled_event_are_exploratory_and_visible() {
      let report = evaluate_worksheet(&fixture("nine-successes-one-cancelled.json"))
          .expect("valid exploratory worksheet");
      assert_eq!(report.classification, EvaluationClass::Exploratory);
      assert_eq!(report.comparable_success_count, 9);
      assert_eq!(report.excluded_event_count, 1);
      assert_eq!(report.outcome_counts.cancelled, 1);
  }

  #[test]
  fn zero_rate_reports_no_github_charge_avoided() {
      let report = evaluate_worksheet(&fixture("zero-rate-public-runner.json"))
          .expect("valid zero-rate worksheet");
      assert_eq!(report.avoided_github_charge_microusd, 0);
  }
  ```

- [ ] **Step 2: Run the boundary tests and confirm they fail**

  Run:

  ```console
  rtk cargo test --locked --test economic_evaluation_contract
  ```

  Expected: FAIL because the fixtures and fail-closed checks are not yet
  implemented.

- [ ] **Step 3: Add deterministic validation and report classifications**

  Extend the Task 1 `EvaluationError` with deterministic English messages and
  use the Task 1 serializable `OutcomeCounts` struct. Validate all optional
  timestamp fields together for success events and enforce:

  ```text
  hosted_dispatched <= hosted_runner_started <= hosted_completed
  local_started <= local_completed <= local_verified
  ```

  Do not calculate a median for failure, cancellation, or incomplete events.
  Do not remove them from the input, report, or denominator for outcome counts.
  Multiply rounded avoided minutes by the rate with `checked_mul`; return an
  error on overflow. A zero rate is valid only as a zero-charge observation.

- [ ] **Step 4: Add the four JSON fixtures**

  Each fixture must declare `schema_version: "economic-evaluation-v1"` and use
  synthetic opaque scope IDs. The invalid-scope fixture changes only one local
  scope ID. The invalid-time fixture makes `local_verified_at_seconds` earlier
  than `local_completed_at_seconds`. The exploratory fixture has nine valid
  success events and one `cancelled` event with no timing fields. The zero-rate
  fixture has ten valid success events and
  `runner_rate_microusd_per_minute: 0`.

  Place this helper before the boundary tests in
  `tests/economic_evaluation_contract.rs`:

  ```rust
  fn fixture(name: &str) -> EvaluationWorksheet {
      let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
          .join("tests/fixtures/economic-evaluation-v1")
          .join(name);
      let bytes = std::fs::read_to_string(path).expect("read fixture");
      serde_json::from_str(&bytes).expect("parse fixture")
  }
  ```

- [ ] **Step 5: Run focused tests, formatter, and commit**

  Run:

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo test --locked --test economic_evaluation_contract
  ```

  Expected: PASS.

  Commit:

  ```console
  rtk git add src/economic_evaluation.rs tests/economic_evaluation_contract.rs tests/fixtures/economic-evaluation-v1
  rtk git commit -m "test: harden economic evaluation boundaries"
  ```

### Task 3: Provide an offline worksheet runner and English evaluation guide

**Files:**
- Create: `examples/evaluate_economic_case_study.rs`
- Modify: `tests/economic_evaluation_contract.rs`
- Create: `docs/TIME_TO_FEEDBACK_EVALUATION.md`
- Modify: `docs/ECONOMIC_QUALIFICATION.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: `EvaluationWorksheet` and `evaluate_worksheet` from Tasks 1–2.
- Produces: one JSON `EvaluationReport` on stdout or a bounded English error on
  stderr with a non-zero exit code.
- Later tasks rely on: English documentation that makes clear that fixtures are
  method tests, not proof of a user's own savings.

- [ ] **Step 1: Write a failing example-process test**

  Add a test that invokes Cargo's example runner against the valid fixture and
  asserts the structured report includes the measurement class and both time
  medians:

  ```rust
  #[test]
  fn example_emits_a_privacy_preserving_json_report() {
      let output = std::process::Command::new(env!("CARGO"))
          .args([
              "run", "--locked", "--quiet", "--example",
              "evaluate_economic_case_study", "--",
              "tests/fixtures/economic-evaluation-v1/valid-ten-events.json",
          ])
          .current_dir(env!("CARGO_MANIFEST_DIR"))
          .output()
          .expect("run example");
      assert!(output.status.success());
      let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
      assert_eq!(report["classification"], "measured");
      assert_eq!(report["hosted_median_end_to_end_seconds"], 180);
      assert_eq!(report["local_median_end_to_end_seconds"], 72);
      assert!(report.get("events").is_none());
  }
  ```

- [ ] **Step 2: Run the test and confirm it fails because the example is absent**

  Run:

  ```console
  rtk cargo test --locked --test economic_evaluation_contract example_emits_a_privacy_preserving_json_report
  ```

  Expected: FAIL because `evaluate_economic_case_study` does not exist.

- [ ] **Step 3: Implement the example as a strictly local runner**

  Implement a one-argument program:

  ```rust
  // usage: cargo run --locked --example evaluate_economic_case_study -- <worksheet.json>
  let mut arguments = std::env::args_os().skip(1);
  let input_path = arguments.next().ok_or("usage: ... <worksheet.json>")?;
  if arguments.next().is_some() { return Err("usage: ... <worksheet.json>".into()); }
  let input = std::fs::read_to_string(input_path)?;
  let worksheet: EvaluationWorksheet = serde_json::from_str(&input)?;
  let report = evaluate_worksheet(&worksheet)?;
  println!("{}", serde_json::to_string_pretty(&report)?);
  ```

  Reject extra arguments. Do not read environment variables, GitHub APIs,
  billing dashboards, Git history, receipt files, or network resources. The
  report must contain aggregates and counts only, never input event IDs,
  commands, repository names, paths, tokens, or timestamps.

- [ ] **Step 4: Write the English method document and update public entry points**

  Create `docs/TIME_TO_FEEDBACK_EVALUATION.md` with these exact sections:

  ```markdown
  # Time-to-feedback evaluation
  ## What this measures
  ## Required comparable-event data
  ## Worksheet format
  ## Running the offline evaluator
  ## Reading the report
  ## Cost is not time
  ## Privacy and evidence limits
  ```

  Document the ten-success threshold, exploratory classification, all six
  measures from the RC.2 design, per-job rounding, retained remote gates,
  local preparation overhead, failure/cancellation visibility, and the fact
  that current Matryca cost examples do not claim measured local timing.

  In `docs/ECONOMIC_QUALIFICATION.md`, add a short “Time-to-feedback” section
  immediately after “What counts as savings”. Link to the new guide and state
  that the August 2026 case studies quantify money/remote compute only unless
  their comparable timing worksheet is published.

  In `README.md`, replace no historical dollar figures. Add one concise
  paragraph below “When CCP actually saves money” linking the guide and saying
  that local CCP can reduce queue and feedback delay only when measured against
  equivalent work; it is not an automatic speed or cost promise.

- [ ] **Step 5: Run focused verification and commit**

  Run:

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo test --locked --test economic_evaluation_contract
  rtk cargo test --locked --test public_documentation_contract
  ```

  Expected: PASS.

  Commit:

  ```console
  rtk git add examples/evaluate_economic_case_study.rs tests/economic_evaluation_contract.rs docs/TIME_TO_FEEDBACK_EVALUATION.md docs/ECONOMIC_QUALIFICATION.md README.md
  rtk git commit -m "docs: add time-to-feedback evaluation guidance"
  ```

### Task 4: Make candidate archives unique and source-bound

**Files:**
- Modify: `scripts/build_release_candidate.sh`
- Modify: `tests/release_hardening_contract.rs`
- Modify: `docs/INSTALLATION.md`

**Interfaces:**
- Consumes: a clean selected checkout, a caller-supplied release label matching
  `v0.1.0-rc.<positive-integer>`, and an absolute empty output directory.
- Produces: `commit-ci-preflight-<release-label>-<target>.tar.gz`,
  `SHA256SUMS`, and in-archive `RELEASE_MANIFEST.json`.
- Later tasks rely on: archive name, label, source SHA, target, and asset
  checksum being independently inspectable without publication.

- [ ] **Step 1: Add a failing release-script contract test**

  Extend `release_candidate_builder_is_local_bounded_and_non_publishing` to
  require all of:

  ```rust
  "usage: scripts/build_release_candidate.sh --release-label v0.1.0-rc.N /absolute/output/directory",
  "release_label=",
  "git rev-parse --verify HEAD",
  "RELEASE_MANIFEST.json",
  "release_label",
  "source_commit",
  "SHA256SUMS",
  "docs/TIME_TO_FEEDBACK_EVALUATION.md",
  "docs/ECONOMIC_QUALIFICATION.md",
  ```

  Also require the forbidden-token list to retain `git push`, `git tag`,
  `cargo publish`, `gh release`, `curl `, `wget `, and `docker `.

- [ ] **Step 2: Run the contract test and confirm it fails**

  Run:

  ```console
  rtk cargo test --locked --test release_hardening_contract release_candidate_builder_is_local_bounded_and_non_publishing
  ```

  Expected: FAIL because RC.2 label and manifest requirements are absent.

- [ ] **Step 3: Implement the non-publishing labelled builder**

  Change the script invocation to:

  ```console
  rtk scripts/build_release_candidate.sh --release-label v0.1.0-rc.2 /absolute/output/directory
  ```

  Validate exactly three arguments, validate the label with the Bash regular
  expression `^v0\.1\.0-rc\.[1-9][0-9]*$`, and preserve the existing absolute
  output-directory and clean-checkout rejection behavior. Resolve the source
  with `git rev-parse --verify HEAD` before compilation. Name the archive:

  ```text
  commit-ci-preflight-<release-label>-<rust-host-target>.tar.gz
  ```

  Before creating the archive, write an in-archive `RELEASE_MANIFEST.json`
  with exactly these public fields. After creating the archive, calculate its
  SHA-256 and write the external checksum manifest:

  ```json
  {
    "release_label": "v0.1.0-rc.2",
    "source_commit": "<40 lowercase hexadecimal characters>",
    "cargo_package_version": "0.1.0",
    "target": "aarch64-apple-darwin",
    "asset_name": "commit-ci-preflight-v0.1.0-rc.2-aarch64-apple-darwin.tar.gz"
  }
  ```

  Because a file cannot self-hash inside its own archive, write
  `archive_sha256` only in the external `SHA256SUMS` file, not in the in-archive
  JSON. Explain this explicitly in the script output and installation guide.
  Package the time and economic guides with the existing support documents.
  `SHA256SUMS` covers maintainer-uploaded archive assets; GitHub-generated
  source archives are bound by their exact public tag and repository reference
  and are not represented as locally generated maintainer assets.

- [ ] **Step 4: Update installation documentation**

  Replace RC.1 wording with RC.2 candidate wording without inserting a future
  release URL until that tag exists. Document the labelled script command,
  `shasum -a 256 -c SHA256SUMS`, extraction of `RELEASE_MANIFEST.json`, and the
  distinction between byte integrity and publisher identity. State that the
  manifest binds source commit and target but does not sign the asset.

- [ ] **Step 5: Run release-contract checks and commit**

  Run:

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

  Expected: PASS.

  Commit:

  ```console
  rtk git add scripts/build_release_candidate.sh tests/release_hardening_contract.rs docs/INSTALLATION.md
  rtk git commit -m "build: bind rc candidate archives to source"
  ```

### Task 5: Align RC.2 status, support, rollback, and changelog copy

**Files:**
- Modify: `README.md`
- Modify: `docs/INSTALLATION.md`
- Modify: `docs/UPGRADE_AND_ROLLBACK.md`
- Modify: `docs/BETA_SUPPORT.md`
- Modify: `CHANGELOG.md`
- Modify: `tests/release_hardening_contract.rs`
- Modify: `tests/public_documentation_contract.rs`

**Interfaces:**
- Consumes: Task 3 time-evaluation guide and Task 4 asset contract.
- Produces: one consistent English RC.2 public story with all current platform
  limitations intact.
- Later tasks rely on: exact wording that a release reviewer can compare with
  the local archive and GitHub release draft.

- [ ] **Step 1: Write failing wording and link tests**

  Add assertions that:

  ```rust
  assert!(README.contains("v0.1.0-rc.2 prerelease"));
  assert!(README.contains("Time-to-feedback evaluation"));
  assert!(INSTALLATION.contains("--release-label v0.1.0-rc.2"));
  assert!(BETA_SUPPORT.contains("v0.1.0-rc.2"));
  assert!(BETA_SUPPORT.contains("Complete project `run` path on Linux x86_64 | `PENDING`"));
  assert!(BETA_SUPPORT.contains("Complete project `run` path on Windows x86_64 | `PENDING`"));
  assert!(!README.contains("guaranteed savings"));
  ```

  Add `docs/TIME_TO_FEEDBACK_EVALUATION.md` to
  `PUBLIC_DOCUMENTS` in `tests/public_documentation_contract.rs` so local links
  are checked.

- [ ] **Step 2: Run the focused documentation tests and confirm they fail**

  Run:

  ```console
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo test --locked --test public_documentation_contract
  ```

  Expected: FAIL because RC.1 copy and the new guide linkage are incomplete.

- [ ] **Step 3: Update only truthful English release copy**

  Update the status lines to say RC.2 is a planned prerelease candidate until
  publication is separately authorized; never say it is published before that
  action completes. Preserve all Linux/Windows full-runtime `PENDING` rows,
  no-signing statements, public-hosted-CI policy, and rollback safety rules.

  Add an RC.2 changelog subsection that describes:

  - the macOS-v5 static-swap policy clarification;
  - the source-bound, checksum-verifiable candidate archive;
  - test-backed cost/time-to-feedback measurement method; and
  - explicit exclusions for universal, net-savings, and public-standard-CI
    monetary claims.

  In the rollback guide, require verification of release label, source commit,
  target, and archive checksum before replacement, then retain the existing
  isolated-prefix and previous-binary preservation flow.

- [ ] **Step 4: Run focused verification and commit**

  Run:

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo test --locked --test public_documentation_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

  Expected: PASS.

  Commit:

  ```console
  rtk git add README.md docs/INSTALLATION.md docs/UPGRADE_AND_ROLLBACK.md docs/BETA_SUPPORT.md CHANGELOG.md tests/release_hardening_contract.rs tests/public_documentation_contract.rs
  rtk git commit -m "docs: prepare rc.2 prerelease boundaries"
  ```

### Task 6: Assemble and inspect one local RC.2 candidate

**Files:**
- Modify: none unless a preceding test identifies a deterministic defect.
- Create outside the repository: one caller-owned empty output directory and
  one caller-owned isolated install prefix.

**Interfaces:**
- Consumes: a clean exact candidate commit after Tasks 1–5, an installed Rust
  toolchain, and the Task 4 release builder.
- Produces: local candidate archive, `SHA256SUMS`, extracted
  `RELEASE_MANIFEST.json`, isolated installation smoke output, and a recorded
  asset SHA-256.
- Later tasks rely on: this is local qualification evidence only. It is not a
  GitHub tag, release, package, signature, or authorization to install it as
  the operator's stable binary.

- [ ] **Step 1: Freeze the candidate identity before building**

  Run:

  ```console
  rtk git status --short --branch
  rtk git rev-parse HEAD
  rtk git diff --check
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

  Expected: clean checkout, one recorded full commit SHA, no whitespace error,
  and current SBOM/notices.

- [ ] **Step 2: Run deterministic source qualification**

  Run:

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  rtk cargo doc --locked --workspace --all-features --no-deps
  rtk cargo test --locked --workspace --all-targets --all-features
  ```

  Expected: PASS. A failure stops this task; do not build a release archive
  from a failing candidate.

- [ ] **Step 3: Build a private local candidate and verify its envelope**

  Choose an empty absolute output directory and run:

  ```console
  rtk scripts/build_release_candidate.sh --release-label v0.1.0-rc.2 /absolute/owned/output
  rtk shasum -a 256 -c /absolute/owned/output/SHA256SUMS
  rtk tar -xzf /absolute/owned/output/commit-ci-preflight-v0.1.0-rc.2-aarch64-apple-darwin.tar.gz -C /absolute/owned/extract
  rtk sed -n '1,120p' /absolute/owned/extract/commit-ci-preflight-v0.1.0-rc.2-aarch64-apple-darwin/RELEASE_MANIFEST.json
  ```

  Expected: checksum PASS and manifest source commit exactly equals the frozen
  candidate commit. Stop if the target differs from the claimed macOS arm64
  target or any packaged document is missing.

- [ ] **Step 4: Smoke-test only an isolated installation**

  Run:

  ```console
  rtk cargo install --locked --path . --root /absolute/owned/rc2-prefix
  rtk /absolute/owned/rc2-prefix/bin/commit-ci-preflight --version
  ```

  Expected: executable runs and reports the candidate version. Do not replace
  `/Users/marco1/.cargo/bin/commit-ci-preflight` or any other stable binary.

- [ ] **Step 5: Record reviewable qualification facts and stop**

  Record the exact HEAD, archive path/basename, complete SHA-256, manifest
  content, source-check results, and isolated-smoke result in the release PR
  draft. Do not tag, push, upload, create a GitHub release, edit remote
  metadata, sign, or publish.

### Task 7: Prepare but do not publish the RC.2 release

**Files:**
- Modify: release PR description only after a separate push/PR authorization.
- Create remotely only after separate authorization: draft pull request, then
  tag and prerelease assets as later distinct owner actions.

**Interfaces:**
- Consumes: Task 6 exact evidence and an independently reviewed branch.
- Produces: a review-ready release PR only. Publication remains out of scope.

- [ ] **Step 1: Obtain a separate authorization for non-force push and draft PR**

  The authorization must bind the branch, exact candidate HEAD, remote base
  HEAD, and explicit stop before tag, release, asset upload, and stable-binary
  replacement.

- [ ] **Step 2: Push and open a draft release PR only if authorized**

  The PR body must include the Task 6 facts and these exact limitations:

  ```text
  This is an unsigned macOS arm64 prerelease candidate.
  It is not a stable release, identity attestation, signing channel, package,
  Windows/Linux runtime qualification, universal time improvement, or net-
  savings guarantee.
  ```

- [ ] **Step 3: Obtain a separate authorization for exact-head qualification**

  Bind any CCP command by worktree, full HEAD, active binary path and SHA-256,
  exact command/launcher, one maximum execution, and a stop before evidence
  publication, ready transition, merge, tag, and release.

- [ ] **Step 4: Obtain final owner authorization only after all release gates pass**

  The final authorization must separately name the exact Git tag, final source
  commit, every local asset SHA-256, GitHub prerelease title/body, and each
  upload. Verify post-upload checksums independently. Stop immediately if any
  source, asset, or release metadata differs.

## Plan Self-Review

### Spec coverage

- Narrow RC.2 scope, unsigned macOS arm64 archive, checksum, SBOM/notices,
  source binding, install/rollback: Tasks 4–6.
- English user-facing explanation: Tasks 3 and 5.
- Cost categories and public-runner no-saving boundary: Tasks 2, 3, and 5.
- Time-to-feedback measures, comparable data, ten-event threshold, ranges,
  local overhead, cancellation/failure visibility: Tasks 1–3.
- No stable/signing/package/Windows/Linux/universal claims: Global Constraints,
  Tasks 4–5, and Task 7 PR wording.
- Separate publication authorization: Task 7.

No specification requirement is unassigned.

### Placeholder scan

No implementation placeholders or generic test steps remain. All new public
Rust types, report fields, fixture roles, test assertions, commands, paths, and
commit boundaries are named explicitly.

### Interface consistency

Tasks 1–3 use one API: `EvaluationWorksheet`, `EvaluationReport`,
`EvaluationClass`, `EvaluationError`, and `evaluate_worksheet`. Tasks 4–6 use
one candidate builder contract:
`--release-label v0.1.0-rc.2 /absolute/output/directory` and one archive naming
rule. No task depends on an undefined CLI command or new dependency.

### Review-focus coverage

All five review-focus items are covered in Task 2 or Task 4 as assigned above.
The example test also verifies that reports omit per-event data, protecting the
privacy boundary specified by Task 3.
