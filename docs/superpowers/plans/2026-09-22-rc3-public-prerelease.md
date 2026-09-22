# v0.1.0-rc.3 Public Prerelease Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prepare a reviewable, unsigned macOS arm64 `v0.1.0-rc.3` candidate
archive with truthful label-neutral source documentation and release evidence.

**Architecture:** Keep product behavior unchanged. Make checked-in public
guidance refer to a downloaded archive's checksum and in-archive manifest,
rather than a mutable current-RC label. Use the existing non-publishing build
script to produce the candidate and verify the archive in an isolated prefix.

**Tech Stack:** Rust 2024, Cargo, Bash, GitHub-hosted CI, Markdown, SPDX 2.3.

**Spec:**
[`docs/superpowers/specs/2026-09-22-rc3-public-prerelease-design.md`](../specs/2026-09-22-rc3-public-prerelease-design.md)

## Global Constraints

- Public prose, release notes, manifests, and examples are English.
- RC.3 is an unsigned macOS arm64 prerelease; it is never a stable release.
- Do not change CLI, receipt, policy, configuration, schema, cache, or
  admission behavior.
- Do not tag, push, upload, publish, sign, install globally, or replace the
  stable executable during local preparation.
- `v0.1.0-rc.2` remains a historical published fact; stale PR #64 and draft
  PR #74 are excluded from RC.3.

## Review Focus

1. A future archive must not contain prose that identifies RC.2 as its own
   release; Task 1 adds deterministic documentation assertions.
2. A changed source commit must invalidate every candidate digest and manifest;
   Task 3 binds and checks the selected exact commit.
3. A checksum that verifies an archive must not be presented as publisher
   identity proof; Task 1 preserves this boundary.
4. A non-empty output directory must fail rather than mix candidate assets;
   Task 2 proves the existing builder boundary.
5. A candidate install must remain prefix-local and rollback must preserve the
   pre-existing binary; Task 3 records both facts.

---

### Task 1: Make release guidance label-neutral

**Files:**

- Modify: `README.md`
- Modify: `docs/INSTALLATION.md`
- Modify: `docs/BETA_SUPPORT.md`
- Modify: `tests/public_documentation_contract.rs`
- Modify: `tests/release_hardening_contract.rs`

**Interfaces:**

- Consumes: the existing archive contract (`SHA256SUMS` and
  `RELEASE_MANIFEST.json`).
- Produces: documentation that directs users to GitHub Releases and verifies
  the exact downloaded archive without naming an obsolete current RC.

- [ ] **Step 1: Write failing documentation-contract assertions**

  Add focused assertions that README and installation guidance link to the
  Releases page, mention both `SHA256SUMS` and `RELEASE_MANIFEST.json`, and do
  not describe `v0.1.0-rc.2` as the current prerelease.

- [ ] **Step 2: Run the focused contract tests**

  Run:

  ```console
  rtk cargo test --locked --test public_documentation_contract
  rtk cargo test --locked --test release_hardening_contract
  ```

  Expected: FAIL only because the new label-neutral assertions are unmet.

- [ ] **Step 3: Replace release-label-specific source prose**

  Keep RC.2 in historical changelog/release-history text. Replace only
  present-tense source guidance with: download the desired prerelease from
  GitHub Releases, verify its adjacent `SHA256SUMS`, inspect its own
  `RELEASE_MANIFEST.json`, then use the archive-specific binary name.

- [ ] **Step 4: Verify and commit the documentation slice**

  Run:

  ```console
  rtk cargo fmt --check
  rtk cargo test --locked --test public_documentation_contract
  rtk cargo test --locked --test release_hardening_contract
  ```

  Commit only the five files listed above with:

  ```console
  rtk git commit -m "docs: make prerelease installation label-neutral"
  ```

### Task 2: Prepare RC.3 source notes and builder boundary

**Files:**

- Modify: `CHANGELOG.md`
- Modify: `tests/release_hardening_contract.rs`
- Modify: `docs/superpowers/specs/2026-09-22-rc3-public-prerelease-design.md`
- Modify: `docs/superpowers/plans/2026-09-22-rc3-public-prerelease.md`

**Interfaces:**

- Consumes: the merged RC.2-to-RC.3 source delta and Task 1 guidance.
- Produces: an English RC.3 candidate note that lists only merged changes and
  preserves distribution and non-claim boundaries.

- [ ] **Step 1: Add a failing textual release-boundary assertion**

  Extend `tests/release_hardening_contract.rs` so a release note cannot claim
  signing, package availability, Windows/Linux runtime qualification, stable
  support, or universal savings.

- [ ] **Step 2: Run the release contract**

  Run:

  ```console
  rtk cargo test --locked --test release_hardening_contract
  ```

  Expected: FAIL because RC.3 notes do not yet exist.

- [ ] **Step 3: Add the RC.3 changelog section**

  Move only the three post-RC.2 merged changes into a dated RC.3 section.
  Do not attribute stale PR #64 or draft PR #74 to RC.3. State the archive
  remains unsigned, macOS arm64 only, prerelease-only, and checksum-verifiable
  rather than identity-attested.

- [ ] **Step 4: Verify and commit the release-note slice**

  Run:

  ```console
  rtk cargo fmt --check
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

  Commit only release-note/spec/plan/test changes with:

  ```console
  rtk git commit -m "docs: prepare rc.3 prerelease notes"
  ```

### Task 3: Build and verify one bounded local candidate

**Files:**

- Create outside the repository: one empty absolute output directory.
- Create outside the repository: one empty isolated install prefix.

**Interfaces:**

- Consumes: clean exact candidate HEAD, label `v0.1.0-rc.3`, and the existing
  `scripts/build_release_candidate.sh` interface.
- Produces: archive, `SHA256SUMS`, manifest inspection record, isolated install
  smoke result, rollback record, and exact SHA-256 values.

- [ ] **Step 1: Capture pre-build identity**

  Record clean `git status --short --branch`, complete `git rev-parse HEAD`,
  `cargo --version`, `rustc -vV`, and the current stable CCP path/hash. Do not
  replace the stable executable.

- [ ] **Step 2: Build the candidate into an empty private output directory**

  Run:

  ```console
  rtk scripts/build_release_candidate.sh --release-label v0.1.0-rc.3 /absolute/empty/output
  ```

  Expected: one `aarch64-apple-darwin` archive and `SHA256SUMS`; no tag, upload,
  package, signature, or global installation.

- [ ] **Step 3: Independently verify the archive**

  Run checksum verification, list archive members, extract into a private
  directory, and compare its `RELEASE_MANIFEST.json` label/source/target/name
  against the recorded candidate identity.

- [ ] **Step 4: Run isolated install and rollback smoke checks**

  Install only into the private prefix using `cargo install --locked --path .
  --root /absolute/private/prefix`; run `--version`; prove the recorded stable
  executable path and SHA-256 are unchanged. Remove only private test-prefix
  content if separately authorized.

- [ ] **Step 5: Run final deterministic verification and record the envelope**

  Run:

  ```console
  rtk cargo fmt --check
  rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  rtk cargo test --locked --workspace --all-targets --all-features
  rtk git diff --check
  ```

  Record exact HEAD, archive SHA-256, manifest bytes, test result, stable-binary
  hash preservation, and explicit NOT-PUBLISHED state. Commit no generated
  archive or private receipt.

### Task 4: Review and publication gate

**Files:**

- No source change required.

**Interfaces:**

- Consumes: Task 1–3 evidence bound to one exact candidate HEAD.
- Produces: a publication envelope for a separate owner authorization.

- [ ] **Step 1: Inspect the complete source diff and candidate assets**

  Confirm RC.3 includes only intended changes, excludes generated archives,
  and has no untracked material proposed for commit.

- [ ] **Step 2: Open a draft PR only after separate push/PR authorization**

  Bind it to the exact candidate HEAD and require hosted CI success. Do not
  tag, upload, or publish.

- [ ] **Step 3: Stop for exact publication authorization**

  Required authorization must name the exact candidate HEAD, tag
  `v0.1.0-rc.3`, archive SHA-256, checksum-manifest SHA-256, GitHub Release
  prerelease state, and stop boundary after independent uploaded-asset digest
  verification.
