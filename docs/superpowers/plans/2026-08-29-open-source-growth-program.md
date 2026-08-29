# Commit CI Preflight Open-Source Growth Programme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the dependency-ordered product, distribution, adoption, qualification, and public-proof milestones required for a truthful Commit CI Preflight stable release.

**Architecture:** `docs/PRODUCT_ROADMAP.md` remains the public product authority; the programme specification defines evidence and governance, while this master plan sequences independently reviewable vertical tranches. Each future tranche starts by refreshing its own exact TDD implementation plan against the then-current `main`, preventing stale source signatures from becoming false instructions.

**Tech Stack:** Rust 1.96.0 (MSRV 1.87), Cargo workspace, shell-free CLI execution, Git/GitHub, Docker-compatible runtimes, GitHub Actions, TOML/JSON/YAML contracts, Markdown documentation, SPDX SBOM, exact-head CCP receipts.

**Spec:** `docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md`

## Global Constraints

- All shell commands begin with `rtk`.
- Preserve the divergent primary checkout; work only in isolated worktrees from a verified `origin/main`.
- TDD is mandatory for behavior and contract changes.
- Every user-visible change updates `CHANGELOG.md`.
- No zero-CI, guaranteed-savings, hosted-parity, A0-identity, or unexecuted-platform claim.
- Never silently replace the installed global CCP producer.
- Keep availability, smoke verification, E2E qualification, A0, and A1 distinct.
- Commit, push, PR, CCP, evidence, ready, merge, release, signing, settings, and case-study publication remain separate gates.
- Deterministic tests precede Docker, native runners, network, and CCP qualification.
- Public evidence excludes raw logs, identities, local paths, secrets, environment values, and customer data.

## Programme File Structure

- `docs/PRODUCT_ROADMAP.md` — public product direction and tranche outcomes.
- `docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md` — canonical execution and evidence contract.
- `docs/superpowers/plans/2026-08-29-open-source-growth-program.md` — dependency order and master checklist.
- `docs/superpowers/plans/YYYY-MM-DD-<milestone>.md` — exact live-source TDD plan created immediately before each milestone.
- `docs/superpowers/goals/2026-08-29-open-source-growth-program.txt` — concise persistent execution pointer.
- `docs/handoffs/OPEN_SOURCE_GROWTH_<date>.md` — restart checkpoint when needed.

---

### Task 0: Commit durable programme control

**Files:**
- Create: `docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md`
- Create: `docs/superpowers/plans/2026-08-29-open-source-growth-program.md`
- Create: `docs/superpowers/goals/2026-08-29-open-source-growth-program.txt`

**Interfaces:**
- Consumes: `docs/PRODUCT_ROADMAP.md`, global CCP operator contract, live `origin/main`.
- Produces: one canonical spec path and one persistent goal used by every later task.

- [ ] **Step 1: Verify programme anchors**

  Run:

  ```console
  rtk git status --short --branch
  rtk git rev-parse HEAD
  rtk git merge-base HEAD origin/main
  rtk gh api repos/MarcoPorcellato/commit-ci-preflight/git/ref/heads/main --jq .object.sha
  ```

  Expected: clean isolated branch, exact base recorded in the specification, no primary-checkout mutation.

- [ ] **Step 2: Self-review the specification and plan**

  Run:

  ```console
  rtk rg -n '^### M[0-9]+|^### Task [0-9]+' docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md docs/superpowers/plans/2026-08-29-open-source-growth-program.md
  ```

  Expected: milestone/task inventory is complete and ordered. Read both files
  end-to-end and reject unresolved markers, vague implementation instructions,
  contradictory interfaces, or a specification requirement without a task.

- [ ] **Step 3: Commit the control artifacts**

  ```console
  rtk git add docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md docs/superpowers/plans/2026-08-29-open-source-growth-program.md docs/superpowers/goals/2026-08-29-open-source-growth-program.txt
  rtk git commit -m "docs: define open-source growth programme"
  ```

### Task 1: M1 conversion surface and dogfooding proof

**Files:**
- Modify: `README.md`
- Modify: `docs/REPOSITORY_PRESENTATION.md`
- Modify: `docs/PRODUCT_ROADMAP.md`
- Modify: `docs/TUTORIAL.md`
- Modify: `CHANGELOG.md`
- Create: `docs/CASE_STUDY_PR71.md`
- Create: `SUPPORT.md`
- Render from: `docs/assets/social-preview.svg`
- Create: `docs/assets/social-preview.png`
- Modify: `.github/ISSUE_TEMPLATE/*.yml` only where live audit finds a concrete funnel gap
- Test: `tests/repository_hygiene_contract.rs`
- Test: `tests/release_hardening_contract.rs`

**Interfaces:**
- Consumes: public PR #71 receipt/gate evidence; current support and release matrices.
- Produces: exact README hero `Run heavy CI locally. Prove the exact commit on GitHub.`, three CTA paths, fit/no-fit decision table, FAQ, dogfooding proof, and metadata change envelope.

- [ ] **Step 1: Create the exact M1 TDD plan**

  Read the live files and write `docs/superpowers/plans/YYYY-MM-DD-m1-conversion-surface.md` with exact line anchors, copy, local-link tests, render command, and commit boundaries. Do not change the README in this step.

- [ ] **Step 2: Add failing contract tests for the new visitor path**

  Extend `tests/repository_hygiene_contract.rs` to require:

  ```rust
  assert!(readme.contains("Run heavy CI locally. Prove the exact commit on GitHub."));
  assert!(readme.contains("See a real receipt"));
  assert!(readme.contains("Check whether CCP fits your repository"));
  assert!(readme.contains("What a receipt proves"));
  assert!(readme.contains("What stays on GitHub"));
  ```

  Add a local Markdown-link walk that rejects missing repository-relative targets without requiring network.

- [ ] **Step 3: Verify the new tests fail for the intended missing anchors**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract -- --nocapture
  ```

  Expected: FAIL only on the newly required conversion anchors or link contract.

- [ ] **Step 4: Implement the benefit-led README path**

  Keep trust boundaries, comparison, cost assumptions, ideal/non-ideal users, security, and documentation links. Remove the duplicated inspection block. Move the admission/resource/`guard exec` deep dive out of the quick-start flow and replace it with one link to `docs/COORDINATION_RUNBOOK.md` and one short safety sentence.

- [ ] **Step 5: Publish a bounded dogfooding case study**

  `docs/CASE_STUDY_PR71.md` must record exact PR head, evidence commit, receipt hash/ID, workflow run, merge commit, what was proven, what was not proven, and no raw/local details. It must not claim savings because no before/after billing evidence exists.

- [ ] **Step 6: Complete community and preview surfaces**

  Add `SUPPORT.md`; review existing bug/feature/adoption forms rather than duplicating them; export the 1280x640 preview to a deterministic PNG under 1 MiB; visually inspect it; record proposed description/topics in `REPOSITORY_PRESENTATION.md`.

- [ ] **Step 7: Run focused validation**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

- [ ] **Step 8: Commit independently reviewable M1 units**

  Use separate commits for contract tests, README/case study, community surface, and preview/metadata. Stop before remote settings, push, PR, or merge unless authorized.

### Task 2: M2 independent verifier boundary

**Files:**
- Create: `docs/superpowers/specs/YYYY-MM-DD-independent-verifier-design.md`
- Create: `docs/superpowers/plans/YYYY-MM-DD-independent-verifier.md`
- Modify: `Cargo.toml`
- Create: `crates/ccp-core/`
- Create: `crates/ccp-verifier/`
- Modify: existing root crate compatibility re-exports
- Test: receipt, verification, schema, CLI, release-hardening and dependency-boundary contracts

**Interfaces:**
- Consumes: canonical receipt/policy/schema behavior and byte fixtures.
- Produces: `ccp_core` types/canonicalization plus a verifier binary exposing bounded verify/schema surfaces without runner dependencies.

- [ ] **Step 1: Freeze the compatibility envelope**

  Record exact fixture hashes and add a dependency-boundary test that fails while verifier code remains coupled to Docker/process/cache/admission/resource modules.

- [ ] **Step 2: Approve the milestone design and exact live-source plan**

  The design must specify crate graph, compatibility re-exports, error stability, binary naming, feature policy, and migration order. The plan must use red/green moves small enough for one reviewer gate.

- [ ] **Step 3: Execute behavior-preserving extraction**

  Move canonical JSON, schemas, receipt, policy, and verification into the inner crates without mixing feature changes. Keep root public behavior stable through explicit re-exports.

- [ ] **Step 4: Prove physical independence**

  ```console
  rtk cargo tree -p ccp-verifier
  rtk cargo test --locked --workspace --all-targets --all-features
  rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  rtk cargo doc --locked --workspace --no-deps
  ```

  Expected: no verifier dependency on runtime, process, workspace, cache, admission, resource, benchmark, or Docker orchestration.

- [ ] **Step 5: Qualify exact head and stop at publication gates**

  Obtain a fresh CCP authorization only after deterministic review. Preserve fixture hashes and rollback path.

### Task 3: M3 multi-platform immutable distribution

**Files:**
- Create: `docs/adr/YYYY-MM-DD-distribution-and-provenance.md`
- Create: `docs/superpowers/plans/YYYY-MM-DD-multi-platform-distribution.md`
- Modify: `Cargo.toml`, release metadata, `docs/INSTALLATION.md`, `docs/BETA_SUPPORT.md`, `CHANGELOG.md`
- Modify/Create: release workflow under `.github/workflows/`
- Create: shell and PowerShell installer tests and scripts
- Create: package metadata for cargo-binstall, Homebrew/tap handoff, and crates.io dry validation
- Test: `tests/release_hardening_contract.rs` and new packaging/install contracts

**Interfaces:**
- Consumes: M2 isolated verifier, exact tag, locked dependency graph.
- Produces: artifact manifest keyed by target with checksum, SBOM, provenance, availability, smoke, and qualification fields.

- [ ] **Step 1: Research and approve one maintained distribution tool**

  Compare current official cargo-dist/release alternatives, pin exact versions and actions by immutable digest/SHA, define rollback, and document why no moving `latest` is trusted.

- [ ] **Step 2: Write failing packaging contracts**

  Require exact target names, archive contents, license/notice/SBOM, checksum manifest, installer checksum enforcement, non-overwrite, isolated prefix, uninstall, and offline failure behavior.

- [ ] **Step 3: Implement artifact matrix and installers without publishing**

  Generate local/hosted candidate artifacts, never tags or registry uploads. Separate runner and verifier packages when trust or size requires it.

- [ ] **Step 4: Native smoke each claimed target**

  Record exact artifact SHA-256, host OS/architecture, installation prefix, `--version`, bounded verifier fixture, uninstall, and rollback. Label unexecuted targets `AVAILABLE` only.

- [ ] **Step 5: Run release qualification and request publication separately**

  No GitHub Release, crate, tap, Winget/Scoop, signature, or attestation publication occurs until its exact artifact set is authorized.

### Task 4: M4 slim trusted GitHub gate

**Files:**
- Create: `docs/superpowers/plans/YYYY-MM-DD-slim-trusted-gate.md`
- Modify: `.github/workflows/receipt-gate.yml`
- Modify: `examples/github/receipt-gate.yml.example`
- Modify: `scripts/github-receipt-gate.sh`
- Modify: `docs/GITHUB_GATE.md`, `docs/THREAT_MODEL.md`, `CHANGELOG.md`
- Test: `tests/github_gate_contract.rs`, `tests/release_hardening_contract.rs`

**Interfaces:**
- Consumes: immutable M3 verifier artifact plus checksum/provenance and rollback artifact.
- Produces: gate inputs `{repository, expected_head, evidence_ref, verifier_digest, policy}` and a bounded exact-head status.

- [ ] **Step 1: Add failing no-build/no-registry gate tests**

  Reject `cargo build`, `cargo run`, Docker, package-registry access, mutable artifact URLs, missing digest verification, and execution of PR code.

- [ ] **Step 2: Implement immutable verifier acquisition**

  Download or restore only the reviewed exact version, verify digest/provenance before execution, and preserve one rollback version.

- [ ] **Step 3: Exercise the complete negative fixture matrix**

  Missing, oversized, stale, malformed, digest-invalid, untrusted, wrong repository,
  wrong head, wrong policy, and wrong configuration must fail closed.

- [ ] **Step 4: Validate hosted behavior on an authorized exact-head PR**

  Record runner allocation separately from verifier wall time and do not infer cost savings from one run.

### Task 5: M5 first-value and adoption commands

**Files:**
- Create: `docs/superpowers/specs/YYYY-MM-DD-first-value-adoption-design.md`
- Create: `docs/superpowers/plans/YYYY-MM-DD-first-value-adoption.md`
- Create: `src/check.rs`, `src/init.rs`, `src/adopt.rs`, `src/setup_github.rs`
- Modify: `src/main.rs`, `src/lib.rs`, config and run orchestration only through reviewed interfaces
- Create: focused CLI/contract tests and Rust/Python/Node fixtures
- Modify: README, tutorial, adoption guide, configuration, compatibility docs, changelog

**Interfaces:**
- `check`: consumes a reviewed plan plus dirty working source; produces local result only, never receipt/evidence/A0.
- `init`: consumes repository inventory; produces deterministic proposed config/policy diff, never silent image-tag resolution.
- `adopt`: consumes `GithubActionsCompatibilityReportV1`; preserves `ready`, `manual_review`, and `blocked` dispositions.
- `setup-github`: consumes reviewed config/policy and immutable verifier coordinates; produces deterministic workflow/policy guidance without overwrite.

- [ ] **Step 1: Specify non-attestante and mutation boundaries**

  Freeze how `check` differs from `run`, how proposals are represented, confirmation semantics, JSON versions, and idempotence.

- [ ] **Step 2: Implement `check` TDD**

  Prove dirty-tree acceptance, result propagation, no receipt, no evidence ref, no publication, and no A0 language.

- [ ] **Step 3: Implement project `init` TDD**

  Detect `Cargo.lock`, Python lockfiles, and Node lockfiles deterministically; emit a diff-first proposal; refuse overwrite; require explicit image digest input or a separately confirmed resolution step.

- [ ] **Step 4: Compose Actions adoption without guessing**

  Reuse the existing parser/report. Unsupported expressions/actions/services remain manual or blocked. Generated proposals are inert until reviewed.

- [ ] **Step 5: Implement deterministic GitHub setup generation**

  Require immutable verifier coordinates, exact policy inputs, no secret values, idempotent output, and explicit overwrite confirmation.

- [ ] **Step 6: Prove the three-language activation path**

  Rust, Python, and Node public clean-room fixtures must reach the documented first result from one page. Docker/network-dependent E2E remains a separately authorized validation.

### Task 6: M6 transactional evidence publication

**Files:**
- Create: milestone design and exact TDD plan
- Create: `src/publish.rs`, `src/hooks.rs`
- Modify: `src/main.rs`, `src/lib.rs`, receipt/verify composition only through stable interfaces
- Create: local bare-remote and hook fixtures/tests
- Modify: GitHub gate, adoption, rollback, troubleshooting, changelog

**Interfaces:**
- `publish(receipt, policy, repository, expected_head, remote) -> PublicationOutcome`
- Exact evidence ref: `refs/heads/ccp-evidence/<64-hex-source-sha>`
- Existing identical ref is idempotent; conflicting content is fail-closed; force push is never used.

- [ ] **Step 1: Approve transaction and recovery design**

  Define remote identity checks, isolated ref construction, interruption journal, retry, cleanup ownership, and unique-work preservation.

- [ ] **Step 2: Implement local bare-repository TDD matrix**

  Cover new ref, identical retry, conflicting ref, stale receipt, dirty/wrong head, non-fast-forward, network error, cancellation, and interrupted cleanup.

- [ ] **Step 3: Add reversible pre-push integration**

  Never overwrite an existing hook; use a documented chain or stop. Uninstall restores only CCP-owned bytes. Source push cannot be reported protected if publication failed.

- [ ] **Step 4: Perform one authorized live pilot**

  Bind exact repo/head/binary/config/generation/output and stop before merge unless separately authorized.

### Task 7: M7 explicit cost intelligence

**Files:**
- Create: milestone design and exact plan
- Create: `src/cost.rs` plus versioned input/output schema
- Modify: `src/main.rs`, `src/lib.rs`, docs and changelog
- Create: golden CSV/JSON fixtures and CLI tests

**Interfaces:**
- Inputs distinguish GitHub usage rows, dated SKU rates, included quota, retained jobs, gate cost, local electricity/hardware rate, and measured duration.
- Outputs label every field `measured`, `supplied`, or `inferred` and never access billing automatically.

- [ ] **Step 1: Freeze schema and privacy limits**
- [ ] **Step 2: TDD zero-savings, under-quota, mixed-platform, rounding, malformed, and large-file cases**
- [ ] **Step 3: Implement bounded local-only analyze/estimate/compare commands**
- [ ] **Step 4: Validate current pricing references against official GitHub documentation**
- [ ] **Step 5: Publish examples only with assumptions visibly attached**

### Task 8: M8 native Linux x86_64 qualification

**Files:**
- Create: native qualification plan, immutable manifest, receipts and public bounded report
- Modify: `docs/BETA_SUPPORT.md`, benchmark/parity, installation and changelog only after evidence
- Extend: native workflow/fixtures only where they reflect genuine host execution

**Interfaces:**
- Consumes exact M3-M6 artifacts and policies.
- Produces native Linux evidence classes for PASS, FAIL, timeout, cancellation, cleanup, cache, verify, publish, and gate.

- [ ] **Step 1: Freeze exact native host/runtime/image/source envelope**
- [ ] **Step 2: Run deterministic synthetic and failure-path checks first**
- [ ] **Step 3: Obtain one-attempt exact authorization for each heavy qualification family**
- [ ] **Step 4: Preserve terminal receipts and independent verification**
- [ ] **Step 5: Promote support wording only after every required class passes**

### Task 9: M9 optional A1 signer identity

**Files:**
- Create: signing/key-custody ADR, threat-model update, milestone design/plan
- Modify: core/verifier policy schemas through a backwards-compatible envelope
- Create: cryptographic fixture tests and documentation

**Interfaces:**
- Detached signed envelope references canonical receipt digest and signer-policy fields.
- Unsigned A0 remains valid where policy permits; A1 never implies A2/A3.

- [ ] **Step 1: Evaluate DSSE/in-toto/Sigstore interoperability from official specifications**
- [ ] **Step 2: Approve credential and revocation model before creating any key**
- [ ] **Step 3: TDD valid, tampered, unknown, expired, revoked, offline, and unsigned-policy paths**
- [ ] **Step 4: Qualify compatibility and negative claims**
- [ ] **Step 5: Publish identity material only with separate exact authorization**

### Task 10: M10 external case studies and stable release

**Files:**
- Create: case-study protocol/template, three consented reports, launch article, stable checklist
- Modify: README, support matrix, installation, upgrade/rollback, roadmap, changelog
- Create/Modify: exact release workflow and artifact manifest from qualified M3 machinery

**Interfaces:**
- Each case study binds repository/source interval, consent, retained remote jobs,
  local runtime, gate runtime, list/billed assumptions, and exact receipts.
- Stable release consumes only previously qualified exact artifacts or performs a fresh exact rebuild with equivalent evidence.

- [ ] **Step 1: Obtain explicit consent and freeze the case-study methodology**
- [ ] **Step 2: Run small, medium/monorepo, and agent-driven studies without fabricated controls**
- [ ] **Step 3: Independently review claims, anonymization, and reproducibility**
- [ ] **Step 4: Execute the stable-release checklist on exact head**
- [ ] **Step 5: Request separate tag, release, package, signature, and announcement authorization**
- [ ] **Step 6: Verify public assets, install/rollback paths, checksums, attestations, support matrix, and links after publication**

## Final programme audit

- [ ] Re-read every specification completion item and map it to terminal current evidence.
- [ ] Verify no skipped, running, partial, wrong-head, wrong-platform, or stale artifact is represented as PASS.
- [ ] Reconcile live GitHub description, topics, social preview, issues, discussions, releases, packages, PRs, evidence refs, and `main`.
- [ ] Verify the installed global producer separately from merged source.
- [ ] Write a final durable handoff with exact anchors and residual boundaries.
- [ ] Mark the persistent goal complete only when every applicable checklist item is proved.
