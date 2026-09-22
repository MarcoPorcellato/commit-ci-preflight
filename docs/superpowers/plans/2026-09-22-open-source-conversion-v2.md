# Open-source conversion v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Make the published RC.2 repository easier for the right maintainer to understand, evaluate, and adopt without weakening CCP's safety, qualification, privacy, licensing, or economic claims.

**Architecture:** Keep the existing evidence-first documentation as the canonical technical layer. Reorder the root README into a short decision path, then link visitors to existing adoption, installation, cost, time-to-feedback, support, and threat-model documents. Add only a small contributor entry point and deterministic documentation contracts; GitHub settings remain an owner-gated external operation.

**Tech Stack:** Markdown, Rust integration tests, Cargo, GitHub repository metadata.

**Spec:** [`../specs/2026-09-22-open-source-conversion-v2-design.md`](../specs/2026-09-22-open-source-conversion-v2-design.md)

## Global constraints

- All public prose is English and must distinguish facts, examples, proposals, and unknowns.
- Preserve the currently published `v0.1.0-rc.2` release boundary: unsigned macOS arm64 archive; no package registry, Homebrew formula, signing, universal performance result, universal monetary result, or Linux/Windows complete-runtime claim.
- Do not claim monetary savings for standard GitHub-hosted public-repository CI. Economic qualification remains private-workload-specific and evidence-bound.
- Do not add a security-reporting channel, Code of Conduct, Discussions, funding link, citation file, or GitHub setting unless its owner-operated process has separately been defined and authorized.
- Do not alter CLI, receipt, schema, workflow execution, or CCP admission behavior in this documentation slice.
- Every new relative Markdown link must be covered by the existing local-link contract.

## Review focus

- A first-time visitor must find the value, the intended user, the first safe action, and the non-fit boundary before reaching implementation detail.
- A public visitor must never infer that local CCP runs are free, equivalent to GitHub Actions, a replacement for review, or proof of identity.
- Release-state language must agree in the README, installation, beta-support, and release hardening contract.
- Contributor guidance must route sensitive reports correctly without inventing availability guarantees.

---

## Task 1: Correct the published RC.2 state with a failing documentation contract

**Files:**
- Modify: `tests/release_hardening_contract.rs`
- Modify: `README.md`
- Modify: `docs/INSTALLATION.md`
- Modify: `docs/BETA_SUPPORT.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Write the failing release-state assertions.**

  In `beta_documents_keep_release_and_security_boundaries_explicit`, replace assertions that require `planned`, `not a downloadable release`, and `PLANNED_RC` with an explicit published-RC.2 contract. Assert all four public documents contain `v0.1.0-rc.2`, installation documents the GitHub Release plus unsigned macOS arm64 boundary, and beta support uses `PUBLISHED_PRERELEASE` (or a similarly unambiguous existing-table value). Assert the old planned-only phrases do not appear.

- [ ] **Step 2: Run the focused test and confirm RED.**

  Run:

  ```console
  cargo test --locked --test release_hardening_contract beta_documents_keep_release_and_security_boundaries_explicit
  ```

  Expected: failure because the public docs still describe RC.2 as unpublished.

- [ ] **Step 3: Update the four public state surfaces.**

  - In `README.md`, describe RC.2 as the current published prerelease, link the exact GitHub release page, and retain the unsigned/archive/platform limits.
  - In `docs/INSTALLATION.md`, make release-asset verification the first installation path; retain reviewed-source installation as an alternative and preserve byte-integrity-versus-signature language.
  - In `docs/BETA_SUPPORT.md`, update only the release-state row and prose needed to match publication; preserve all pending platform and security boundaries.
  - In `CHANGELOG.md`, add one concise Unreleased documentation entry explaining the published-RC.2 state correction.

- [ ] **Step 4: Run the focused test and confirm GREEN.**

  Run the Step 2 command again. Expected: pass.

## Task 2: Restructure README around a bounded decision journey

**Files:**
- Modify: `README.md`
- Modify: `tests/release_hardening_contract.rs`
- Modify: `tests/public_documentation_contract.rs`

- [ ] **Step 1: Add the failing human-first navigation contract.**

  Extend `public_readme_is_human_first_and_truthfully_differentiated` to require these exact visible decisions in the README:

  - a concise outcome-led opening;
  - a `## Is CCP for this repository?` heading;
  - a `## Start here` heading;
  - the three path labels `Evaluate cost`, `Try safely`, and `Adopt CCP`;
  - an explicit public-hosted-CI non-economic boundary;
  - `CONTRIBUTING.md` as the contributor route.

  Keep the existing headings and official comparison sources required by the contract unless the spec deliberately replaces them with an equivalent heading and the test is updated in the same change.

- [ ] **Step 2: Run the focused README contract and confirm RED.**

  Run:

  ```console
  cargo test --locked --test release_hardening_contract public_readme_is_human_first_and_truthfully_differentiated
  ```

  Expected: failure until the decision journey is present.

- [ ] **Step 3: Rewrite only the README information architecture.**

  Preserve technical truth and existing primary sections, but place the following before deep implementation detail:

  1. One sentence: heavy local checks, exact-commit evidence, lightweight remote verification.
  2. One sentence naming maintainers of private repositories with expensive/repeated workflows as the economic target, plus explicit non-economic exceptions.
  3. A compact fit/non-fit table covering public standard hosted CI, private evidence-qualified hosted cost, hardware/data-residency needs, and unsupported parity/security assumptions.
  4. `Start here` with exactly three links: `docs/ECONOMIC_QUALIFICATION.md`, `docs/TUTORIAL.md`, and `docs/ADOPTION_GUIDE.md`.
  5. A short proof-flow explanation that links to threat model and support, rather than repeating their details.

  Do not fabricate shell results, savings figures, compatibility, support response time, or feature parity. Keep case studies clearly labelled as bounded examples.

- [ ] **Step 4: Run focused documentation contracts and link validation.**

  Run:

  ```console
  cargo test --locked --test release_hardening_contract public_readme_is_human_first_and_truthfully_differentiated
  cargo test --locked --test public_documentation_contract current_public_documents_have_valid_local_links
  ```

  Expected: both pass.

## Task 3: Add a minimal contributor entry point and route existing forms

**Files:**
- Create: `CONTRIBUTING.md`
- Modify: `README.md`
- Modify: `tests/public_documentation_contract.rs`
- Modify: `tests/release_hardening_contract.rs`

- [ ] **Step 1: Add failing contracts for the contributor route.**

  Add `CONTRIBUTING.md` to `PUBLIC_DOCUMENTS` in `current_public_documents_have_valid_local_links`. Add assertions that it links to `README.md`, `docs/THREAT_MODEL.md`, `SECURITY.md`, existing issue forms, and the PR template without claiming a guarantee that GitHub cannot provide.

- [ ] **Step 2: Run the focused test and confirm RED.**

  Run:

  ```console
  cargo test --locked --test public_documentation_contract current_public_documents_have_valid_local_links
  ```

  Expected: failure because `CONTRIBUTING.md` does not exist.

- [ ] **Step 3: Write `CONTRIBUTING.md` in plain English.**

  Include:

  - the project's evidence-first contribution philosophy;
  - a minimal local deterministic check sequence with no claim that it qualifies a native Docker path;
  - how to choose the existing bug, feature, and adoption-help forms;
  - a reminder to avoid secrets, private receipts, raw logs, proprietary source, and personal data in public reports;
  - a link to `SECURITY.md` for vulnerabilities;
  - a focused-then-full-test expectation and pull-request scope expectation;
  - a link to the existing PR template, without duplicating it.

  Link it from the README contribution/support area.

- [ ] **Step 4: Run contributor and README contract checks.**

  Run:

  ```console
  cargo test --locked --test public_documentation_contract current_public_documents_have_valid_local_links
  cargo test --locked --test release_hardening_contract public_readme_is_human_first_and_truthfully_differentiated
  ```

  Expected: both pass.

## Task 4: Verify the documentation surface and capture an owner-gated metadata proposal

**Files:**
- Modify: `docs/REPOSITORY_PRESENTATION.md`
- Modify: `CHANGELOG.md`
- Modify: `tests/public_documentation_contract.rs`

- [ ] **Step 1: Add a failing documentation invariant.**

  Extend the public-document list to include `docs/REPOSITORY_PRESENTATION.md` if not already covered by a direct semantic check. Add a narrow assertion that the document distinguishes source-controlled presentation from owner-only GitHub settings.

- [ ] **Step 2: Run the focused test and confirm RED only if the stated boundary is absent.**

  Run:

  ```console
  cargo test --locked --test public_documentation_contract current_public_documents_have_valid_local_links
  ```

  If the baseline is already green, preserve the existing valid boundary and do not create an artificial red test. Record that the test-first criterion was already satisfied by prior coverage.

- [ ] **Step 3: Document, do not mutate, the metadata follow-up.**

  Update `docs/REPOSITORY_PRESENTATION.md` with a compact owner checklist for a later read-only audit of description, topics, website, social preview, Discussions, security reporting, and community-health files. State that each external setting change needs a separate owner authorization and live verification.

- [ ] **Step 4: Verify local links and public-boundary assertions.**

  Run the Step 2 command and any newly added focused semantic test. Expected: pass.

## Task 5: Whole-slice review, verification, and handoff gate

**Files:**
- Review: every file changed by Tasks 1–4

- [ ] **Step 1: Inspect the final diff against the exact base.**

  Run:

  ```console
  git diff --check e4b05c13bef4ed2e458f7e09551618304039b21b...HEAD
  git diff -- e4b05c13bef4ed2e458f7e09551618304039b21b...HEAD -- README.md CONTRIBUTING.md docs/INSTALLATION.md docs/BETA_SUPPORT.md docs/REPOSITORY_PRESENTATION.md CHANGELOG.md tests/public_documentation_contract.rs tests/release_hardening_contract.rs
  ```

  Verify no private paths, user names, raw local commands, credentials, receipts, cached outputs, or unproven quantitative claims reached public prose.

- [ ] **Step 2: Run deterministic verification.**

  Run:

  ```console
  cargo fmt --check
  cargo test --locked --test public_documentation_contract
  cargo test --locked --test release_hardening_contract
  cargo test --locked --workspace --all-targets --all-features
  ```

  Expected: all pass. No CCP run, Docker workload, release, tag, package, push, PR, or GitHub settings mutation is included in this plan.

- [ ] **Step 3: Request a fresh independent review.**

  Review for claim inflation, contradictory release status, broken local links, accidental privacy leakage, and whether the README still serves existing technical users. Resolve only findings backed by the changed source and deterministic checks.

- [ ] **Step 4: Commit locally and stop at the publication gate.**

  Create one or more conventional, reviewable local commits. Record exact HEAD, test outputs, and a concise PR summary. Stop before any push, PR creation, GitHub metadata change, hosted workflow dispatch, CCP action, release, tag, installation, or cleanup until explicitly authorized.

## External owner gate after source review

The following is intentionally outside the local documentation implementation:

1. Read-only audit the live GitHub About description, topics, website, social-preview state, community profile, PR/issue form rendering, and required checks.
2. Compare each value against `docs/REPOSITORY_PRESENTATION.md` and the merged source only.
3. Ask the owner for one exact authorization listing the settings that would change.
4. Apply only those settings, then verify them read-only and record the result.

No repository setting is implied by a documentation merge.
