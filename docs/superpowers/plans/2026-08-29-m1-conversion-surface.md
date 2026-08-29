# M1 Conversion Surface and Dogfooding Proof Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Commit CI Preflight understandable, credible, and actionable for a new maintainer while preserving every evidence, release, and trust boundary.

**Architecture:** Human copy is reviewed as copy, not frozen by source-string tests. Executable repository contracts instead protect navigability, issue-form validity, and the rendered social-preview asset; a bounded dogfooding case study connects the product claim to public exact-head evidence. GitHub metadata changes remain a separate remote gate after the source PR is merged.

**Tech Stack:** Markdown, Rust integration tests, GitHub issue forms, SVG/PNG assets, GitHub CLI read-only evidence checks.

**Spec:** `docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md`

## Global Constraints

- All shell commands begin with `rtk`.
- Work only in `/Users/marco1/Documents/CODICE con VS CODE/ccp-worktrees/open-source-growth-program-v1` on `codex/open-source-growth-program-v1`.
- Preserve the divergent primary checkout.
- Do not assert exact marketing sentences in tests; human prose earns editorial review, not a change-detector test.
- TDD applies to the executable link, PNG, and issue-form contracts.
- Every public claim names or links authoritative evidence and states what it does not prove.
- Do not claim zero CI, guaranteed savings, publisher identity, execution attestation, hosted parity, or unqualified platforms.
- Every user-visible source change updates `CHANGELOG.md`.
- Do not push, open a PR, publish evidence, merge, alter GitHub settings, or upload the preview during local M1 implementation.
- Do not invoke CCP, Docker, or a heavy full-suite run without a fresh exact authorization envelope.

## File Structure

- `tests/public_documentation_contract.rs` — executable repository-relative Markdown link and PNG structure validation.
- `README.md` — visitor journey: pain, mechanism, proof, fit, first action, boundaries.
- `docs/CASE_STUDY_PR71.md` — exact public dogfooding proof with explicit non-claims.
- `SUPPORT.md` — routes adoption help, defects, security reports, and evidence discussions.
- `.github/ISSUE_TEMPLATE/adoption_help.yml` — structured adoption question form distinct from evidence reports.
- `tests/repository_hygiene_contract.rs` — issue-form and source social-preview safety contract.
- `docs/assets/social-preview.svg` — editable source of truth.
- `docs/assets/social-preview.png` — GitHub-uploadable 1280x640 render under 1 MiB.
- `docs/REPOSITORY_PRESENTATION.md` — exact proposed description/topics and owner-only upload procedure.
- `CHANGELOG.md` — Unreleased user-visible M1 record.

---

### Task 1: Executable public-documentation contracts

**Files:**
- Create: `tests/public_documentation_contract.rs`

**Interfaces:**
- Produces: `local_link_destinations(markdown: &str) -> Vec<String>`
- Produces: `validate_local_links(root: &Path, document: &Path) -> Vec<String>`
- Produces: `markdown_heading_anchors(markdown: &str) -> HashSet<String>`
- Produces: `png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String>`
- Consumed by: Task 2 public README/case-study/support link validation and Task 3 preview validation.

- [ ] **Step 1: Write failing parser and PNG tests**

  Create the test file with literal fixtures that require:

  ```rust
  #[test]
  fn local_link_validation_reports_missing_files_and_fragments() {
      let root = unique_fixture_root("broken-links");
      std::fs::create_dir_all(root.join("docs")).expect("create docs");
      std::fs::write(
          root.join("README.md"),
          "# Root\n\n[missing](docs/missing.md)\n[bad anchor](docs/present.md#absent)\n",
      )
      .expect("write README");
      std::fs::write(root.join("docs/present.md"), "# Present heading\n")
          .expect("write target");

      let findings = validate_local_links(&root, Path::new("README.md"));

      assert_eq!(
          findings,
          vec![
              "README.md: missing local target docs/missing.md",
              "README.md: missing fragment #absent in docs/present.md",
          ]
      );
  }

  #[test]
  fn png_dimensions_rejects_invalid_or_truncated_bytes() {
      assert_eq!(png_dimensions(b"not a png"), Err("invalid PNG signature".into()));
  }
  ```

  Test fixtures must use a unique directory under `std::env::temp_dir()` and remove only that exact owned directory after assertions.

- [ ] **Step 2: Run RED and confirm the intended compile failure**

  ```console
  rtk cargo test --locked --test public_documentation_contract -- --nocapture
  ```

  Expected: compilation fails because the four declared helper interfaces are not implemented.

- [ ] **Step 3: Implement the minimal link and PNG helpers**

  Implement only inline Markdown links of the form `](destination)`. Ignore `http:`, `https:`, `mailto:`, absolute `/` URLs, and fragment-only links after validating them against the current document. Strip optional angle brackets and split one optional `#fragment`. Reject path traversal escaping `root`; percent-decoding is out of scope and must be reported as unsupported rather than guessed.

  Parse PNG dimensions only from the standard 8-byte signature followed by the `IHDR` chunk. Read width and height as big-endian `u32`; reject missing `IHDR`, zero dimensions, and truncated bytes.

- [ ] **Step 4: Add the repository public-document set**

  Validate these current files:

  ```rust
  const PUBLIC_DOCUMENTS: &[&str] = &[
      "README.md",
      "SUPPORT.md",
      "docs/CASE_STUDY_PR71.md",
      "docs/INSTALLATION.md",
      "docs/TUTORIAL.md",
      "docs/ADOPTION_GUIDE.md",
      "docs/BETA_SUPPORT.md",
      "docs/REPOSITORY_PRESENTATION.md",
      "docs/THREAT_MODEL.md",
  ];
  ```

  Until Task 2 creates `SUPPORT.md` and the case study, keep them out of the active slice but leave one comment naming Task 2 as the point where they become required. The completed Task 2 must remove the comment and add both paths.

- [ ] **Step 5: Run GREEN**

  ```console
  rtk cargo test --locked --test public_documentation_contract -- --nocapture
  ```

  Expected: all parser, PNG, traversal, and current-document link tests pass.

- [ ] **Step 6: Commit Task 1**

  ```console
  rtk git add tests/public_documentation_contract.rs
  rtk git commit -m "test: validate public documentation artifacts"
  ```

### Task 2: Benefit-led README, support path, and PR #71 case study

**Files:**
- Modify: `README.md`
- Create: `docs/CASE_STUDY_PR71.md`
- Create: `SUPPORT.md`
- Modify: `tests/public_documentation_contract.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1 link checker.
- Consumes exact public evidence: PR head `f3fb14a0329cf031d08f474115a8b56fe1cffcf6`, evidence commit `382ba81a6869777a92f987068e2814f542e7ec8b`, receipt SHA-256 `624477439b1bdb7b397876a7e5f8434057f8805835162c5fccb21beac015000b`, receipt ID `sha256:3536dce784a646f6450f6a228230e09ffff8d4d5bd45f89dfd285223a9a235d1`, workflow run `33220815044`, merge commit `820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc`.
- Produces: public visitor path and dogfooding proof consumed by Task 4 editorial review.

- [ ] **Step 1: Reverify the public case-study anchors read-only**

  ```console
  rtk gh pr view 71 --repo MarcoPorcellato/commit-ci-preflight --json state,headRefOid,mergeCommit,statusCheckRollup,url
  rtk gh api repos/MarcoPorcellato/commit-ci-preflight/git/ref/heads/ccp-evidence/f3fb14a0329cf031d08f474115a8b56fe1cffcf6 --jq .object.sha
  ```

  Stop Task 2 if any exact anchor differs; do not repair or reinterpret remote evidence.

- [ ] **Step 2: Restructure the first visitor journey**

  Use this hierarchy without changing the established trust claim:

  1. hero: `Run heavy CI locally. Prove the exact commit on GitHub.`;
  2. one short mechanism paragraph;
  3. three links: real receipt/case study, clean-room demo, repository-fit decision;
  4. current prerelease warning;
  5. problem and four-stage local-to-GitHub flow;
  6. compact `Is CCP for this repository?` yes/no table;
  7. short first inspection;
  8. dogfooding proof;
  9. differentiation, assumptions, boundaries, security, deeper docs.

  Preserve the existing official comparison links and the `not an identity attestation` statement. Keep `The problem`, `How it works`, `Quick start`, `What makes it different`, `When to use it`, `When not to use it`, and `Evidence and limitations` headings required by the current release contract.

- [ ] **Step 3: Remove the quick-start interruption**

  Delete the duplicated plan/doctor/dry-run block. Move the long admission, resource, swap, history, and `guard exec` explanation out of the quick-start flow by replacing it with a concise operator-safety paragraph linking to `docs/COORDINATION_RUNBOOK.md`, `docs/LOCAL_RUN.md`, and `docs/RESOURCE_OBSERVATION_HISTORY.md`. Do not delete those source contracts.

- [ ] **Step 4: Write the bounded PR #71 case study**

  Include:

  - problem: copied dry-run Docker argv is not a replay bundle;
  - exact source/evidence/workflow/merge anchors;
  - lifecycle: local qualification, append-only evidence, exact-head GitHub verification, merge;
  - proof: receipt integrity/policy and exact-head gate;
  - non-proofs: savings, producer identity, arbitrary hosted parity, other platforms;
  - links to the PR, workflow, evidence ref, receipt spec, and verification policy.

  Exclude local paths, commands, raw logs, usernames beyond public GitHub ownership, environment values, or machine identity.

- [ ] **Step 5: Write `SUPPORT.md`**

  Route:

  - reproducible defects to the bug form;
  - adoption questions to the adoption-help form created in Task 3;
  - completed trials to the adoption-report form;
  - security vulnerabilities to `SECURITY.md` and private reporting instructions;
  - design proposals to the feature form.

  State that public issues must not contain secrets, proprietary logs, receipts with private fields, or customer data.

- [ ] **Step 6: Make the new documents part of the executable link gate**

  Add `SUPPORT.md` and `docs/CASE_STUDY_PR71.md` to `PUBLIC_DOCUMENTS`. Run:

  ```console
  rtk cargo test --locked --test public_documentation_contract -- --nocapture
  rtk cargo test --locked --test release_hardening_contract public_readme_is_human_first_and_truthfully_differentiated -- --exact
  ```

- [ ] **Step 7: Update the Unreleased changelog**

  Add one concise `Added` entry covering the visitor journey, bounded PR #71 case study, and support routing. Do not claim GitHub metadata or social-preview upload is already live.

- [ ] **Step 8: Commit Task 2**

  ```console
  rtk git add README.md SUPPORT.md docs/CASE_STUDY_PR71.md tests/public_documentation_contract.rs CHANGELOG.md
  rtk git commit -m "docs: sharpen the public CCP adoption path"
  ```

### Task 3: Community intake and uploadable social preview

**Files:**
- Create: `.github/ISSUE_TEMPLATE/adoption_help.yml`
- Modify: `tests/repository_hygiene_contract.rs`
- Create: `docs/assets/social-preview.png`
- Modify: `tests/public_documentation_contract.rs`
- Modify: `docs/REPOSITORY_PRESENTATION.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 2 `SUPPORT.md` link to the adoption-help form.
- Produces: safe structured help intake, uploadable 1280x640 preview, and exact owner metadata proposal.

- [ ] **Step 1: Write failing issue-form and PNG requirements**

  In `tests/repository_hygiene_contract.rs`, add:

  ```rust
  const ADOPTION_HELP_FORM: &str =
      include_str!("../.github/ISSUE_TEMPLATE/adoption_help.yml");
  ```

  Require one YAML mapping with `name`, `description`, `title`, `labels`, and `body`, plus the same unsafe-claim rejection applied to other templates. Add the path to `roadmap_and_templates_reference_existing_local_docs`.

  In `tests/public_documentation_contract.rs`, add `include_bytes!` for `docs/assets/social-preview.png` and require `png_dimensions(...) == Ok((1280, 640))` and `bytes.len() < 1_048_576`.

- [ ] **Step 2: Run RED**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract issue_template_yaml_is_present_and_safe -- --exact
  rtk cargo test --locked --test public_documentation_contract social_preview_png_is_uploadable -- --exact
  ```

  Expected: compile failure because the new form and PNG do not exist.

- [ ] **Step 3: Create the adoption-help form**

  Use label `question`, which already exists remotely. Require runtime/platform, repository language, attempted step, bounded error summary, expected outcome, and confirmation that no secrets/private logs are included. Explain that adoption help is not an evidence report.

- [ ] **Step 4: Render and inspect the preview**

  Render into an owned temporary directory:

  ```console
  rtk qlmanage -t -s 1280 -o /private/tmp/ccp-social-preview-render docs/assets/social-preview.svg
  rtk sips -z 640 1280 /private/tmp/ccp-social-preview-render/social-preview.svg.png --out docs/assets/social-preview.png
  rtk sips -g pixelWidth -g pixelHeight docs/assets/social-preview.png
  rtk ls -lh docs/assets/social-preview.png
  ```

  Inspect the committed PNG visually at original resolution. If Quick Look changes the layout or crops text, stop Task 3 and use a reviewed renderer rather than accepting a distorted asset.

- [ ] **Step 5: Update repository presentation guidance**

  Record the proposed live values exactly:

  - Description: `Run heavy CI locally. Verify exact-commit receipts on GitHub.`
  - Topics: `ci`, `continuous-integration`, `developer-tools`, `devtools`, `github-actions`, `local-ci`, `local-first`, `reproducible-builds`, `rust`, `supply-chain`.

  State that the PNG is an upload candidate only. Preserve the owner-only manual verification steps and the fact that committing metadata guidance changes no GitHub setting.

- [ ] **Step 6: Run GREEN**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract
  rtk cargo test --locked --test public_documentation_contract
  ```

- [ ] **Step 7: Update the Unreleased changelog and commit Task 3**

  Add an `Added` entry for adoption-help intake and the uploadable preview candidate, explicitly not its remote activation.

  ```console
  rtk git add .github/ISSUE_TEMPLATE/adoption_help.yml docs/assets/social-preview.png docs/REPOSITORY_PRESENTATION.md tests/repository_hygiene_contract.rs tests/public_documentation_contract.rs CHANGELOG.md
  rtk git commit -m "docs: complete public intake and preview assets"
  ```

### Task 4: M1 integration review and local qualification

**Files:**
- Modify: `docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md`
- Modify: SDD ledger only in git-ignored workspace

**Interfaces:**
- Consumes: Tasks 1-3 commits and review verdicts.
- Produces: M1 status/evidence update and the exact remote-action envelope for push/PR and later metadata upload.

- [ ] **Step 1: Perform structured editorial acceptance**

  A reviewer must answer from the rendered README without relying on implementation docs:

  1. What expensive duplication does CCP address?
  2. What runs locally and what remains on GitHub?
  3. What does a receipt prove and not prove?
  4. Is the repository a fit?
  5. What is the shortest safe next action?
  6. Which public exact-head example demonstrates the flow?

  Any answer requiring inference or deep-doc navigation is an Important finding.

- [ ] **Step 2: Run focused M1 validation**

  ```console
  rtk cargo test --locked --test public_documentation_contract
  rtk cargo test --locked --test repository_hygiene_contract
  rtk cargo test --locked --test release_hardening_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  rtk git diff --check 820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc...HEAD
  ```

- [ ] **Step 3: Run the broad branch review**

  Review the complete M1 diff for claim accuracy, broken links, accidental release-state changes, private evidence, duplicated onboarding, accessibility, issue-form safety, and asset legibility. One fix wave and one scoped re-review are allowed by SDD.

- [ ] **Step 4: Update the canonical milestone status**

  Change only the M1 current-status/evolution-ledger fields that terminal local evidence proves. Do not mark GitHub metadata, social-preview activation, push, PR, CCP receipt, or merge complete.

- [ ] **Step 5: Commit the local M1 closure**

  ```console
  rtk git add docs/superpowers/specs/2026-08-29-open-source-growth-program-design.md
  rtk git commit -m "docs: record local M1 qualification"
  ```

- [ ] **Step 6: Stop at the external gate**

  Prepare but do not execute:

  - non-forced branch push;
  - PR creation;
  - exact-head CCP qualification envelope;
  - evidence publication and merge;
  - GitHub description/topics update;
  - social-preview upload and live-page verification;
  - creation of missing remote labels such as `adoption`, `security`, `cost-model`, and `platform`.
