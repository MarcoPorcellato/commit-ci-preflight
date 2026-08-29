---
type: execution-specification
title: "Commit CI Preflight open-source growth and stable-release programme"
description: "Turn CCP's evidence-first technical core into an understandable, installable, independently verifiable, and responsibly promotable open-source product."
status: approved-for-staged-delivery
last_verified: 2026-08-29
---

# Commit CI Preflight open-source growth and stable-release programme

This file is the canonical execution contract for the programme. The public
product direction remains authoritative in [`PRODUCT_ROADMAP.md`](../../PRODUCT_ROADMAP.md);
this specification defines how that direction is delivered, evidenced, and
resumed across multiple pull requests. Conversation history, issues, and local
notes are supporting context, not competing plans.

## Outcome

Commit CI Preflight becomes a public open-source product for maintainers who
repeat deterministic heavy CI work on paid hosted runners. A new maintainer can:

1. understand the problem and the local/remote trust split in 30 seconds;
2. decide whether CCP fits their repository without reading implementation docs;
3. install a checksum- and provenance-verifiable artifact for a supported host;
4. reach a first non-attestante development check with a short, deterministic path;
5. produce and independently verify an exact-commit A0 receipt in a clean-room path;
6. connect the slim GitHub exact-head gate without compiling the runner in the gate;
7. publish evidence through a transactional, append-only workflow;
8. evaluate cost and latency using explicit measured or supplied inputs;
9. distinguish availability, smoke verification, end-to-end qualification, and identity assurance;
10. inspect reproducible external case studies before a truthful stable release.

The programme is complete only when the final checklist is proven against live
GitHub state, native receipts, released artifacts, and user-visible instructions.

## Authoritative anchors

| Item | Verified state | Evidence |
|---|---|---|
| Repository | `MarcoPorcellato/commit-ci-preflight` | live GitHub repository |
| Programme base | `820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc` | `origin/main`, post-PR #71 |
| Delivery branch | `codex/open-source-growth-program-v1` | isolated worktree |
| Isolated worktree | `/Users/marco1/Documents/CODICE con VS CODE/ccp-worktrees/open-source-growth-program-v1` | clean at programme creation |
| Product authority | `docs/PRODUCT_ROADMAP.md` | staged PR 1-10 roadmap |
| Current public release | `v0.1.0-rc.1` | unsigned macOS arm64 archive plus checksum |
| Current release drift | public tag is 39 commits behind `main` at programme creation | GitHub compare API |
| Active global producer | source `faf587890e4f899803f027660bc66452623f405e`, binary SHA-256 `7cde4c2888721d72fbb8c86b4fdcc75f992050979c5175a5bf10b0cecfa7c6f8` | live global CCP operator contract, reverified 2026-08-29 |
| Current programme milestone | M0, in delivery | this specification and master plan |

Every drift-prone anchor must be reverified before a commit, push, CCP
qualification, evidence publication, repository-setting mutation, release, or
merge. A merged source change does not update the active global producer.

## Status vocabulary

- **Verified:** terminal authoritative evidence exists for the exact artifact or commit.
- **In delivery:** recoverable saved work exists but qualification is incomplete.
- **Blocked:** a named human, platform, secret, consent, or external dependency prevents this path.
- **Planned:** dependency-ordered work with an explicit exit gate.
- **Deferred:** intentionally excluded until a predecessor justifies it.
- **Available:** an artifact exists for a target; no smoke or end-to-end claim is implied.
- **Smoke verified:** installation and bounded CLI probes passed on the named target.
- **End-to-end qualified:** genuine native runner-to-receipt-to-verifier evidence passed on the named target.
- **A0:** receipt integrity and repository-policy assurance only.
- **A1:** policy-accepted signer identity; no execution-truth claim.

## Scope

### Public conversion surface

- Benefit-led README hero, clear calls to action, fit/no-fit decision path, FAQ,
  dogfooding proof, and a short path to first visible value.
- Accurate GitHub description, topics, social preview, support and contributor pathways.
- Public claims bound to exact receipts, artifacts, platforms, dates, and assumptions.

### Product activation

- Physically independent verifier and slim trusted GitHub gate.
- Multi-platform artifact generation with checksums, SBOM, provenance, installers,
  package-manager metadata, smoke verification, and explicit publication gates.
- Non-attestante dirty-tree `check`, deterministic project `init`, bounded Actions
  adoption proposal, GitHub setup generation, and clean-room demonstrations.
- Transactional evidence publication and reversible pre-push integration.
- Explicit cost analysis and comparison from user-supplied or measured inputs.

### Qualification and responsible promotion

- Native Linux x86_64 complete `run` qualification independent of fixed benchmark evidence.
- Optional A1 signed identity envelope that remains distinct from execution attestation.
- Three consented, reproducible case studies and a stable-release launch package.

## Non-goals

- Claiming zero remote CI, guaranteed savings, GitHub-hosted parity, producer
  identity at A0, or truthful execution against a malicious insider.
- Executing arbitrary marketplace Actions or silently translating unsupported workflow features.
- Treating availability, compilation, benchmark evidence, or containerized Linux
  execution on macOS as native Linux end-to-end qualification.
- Publishing a crate, package, signature, key, release, case study, billing data,
  or customer information without its separate authority and evidence gate.
- Replacing the installed global CCP producer merely because newer source is merged.
- Manipulating admission, journal, receipt, lease, cache, lock, or resource state to make progress.

## Invariants and approval boundaries

1. All shell commands begin with `rtk`.
2. Dirty or divergent user checkouts remain untouched; delivery uses isolated worktrees.
3. TDD applies to behavior and contract changes: failing test, minimal implementation,
   focused PASS, then proportional broad validation.
4. Every user-visible change updates `CHANGELOG.md`.
5. Deterministic checks run before Docker, network, native qualification, or CCP.
6. `check` can never create an A0 receipt or publish evidence.
7. Unsupported Actions remain `manual_review` or `blocked`, never guessed into executable configuration.
8. Generated files are deterministic, diff-first, idempotent, and never overwrite without explicit confirmation.
9. Evidence refs are exact-SHA-derived and append-only; no force push belongs to the standard path.
10. A checksum or SBOM proves no publisher identity. A signature identifies a declarant but proves no execution truth.
11. Commit, push, PR creation, ready transition, CCP run, evidence publication,
    merge, repository-setting mutation, package upload, release, signing, and
    case-study publication are distinct gates.
12. Heavy CCP authorizations remain exact-worktree, exact-head, binary-hash,
    configuration-digest, generation, maximum-count, and stop-boundary bound.
13. Public metrics must be measured, supplied, or explicitly inferred; fabricated
    adoption, savings, timing, billing, or platform evidence is prohibited.
14. Raw logs, usernames, local paths, environment values, container IDs, secrets,
    proprietary data, and personal identity fields do not enter public evidence.

## Evidence and claim envelope

| Evidence | Proves | Does not prove |
|---|---|---|
| Deterministic unit/contract tests | exact tested behavior on the named source/toolchain | native runtime, publication, or another commit |
| Hosted GitHub check | exact hosted job result and artifact for its SHA | unexecuted platforms or local producer identity |
| CCP receipt + independent verify | exact receipt integrity and configured policy match | who ran it or malicious-host truthfulness |
| Native installation smoke | named artifact installs and bounded probes pass on the named host | full project `run` qualification |
| Native end-to-end receipt | named platform completed runner/receipt/verifier path | every platform or every repository |
| Signed A1 envelope | accepted signer declared the receipt | attested execution or honest insider behavior |
| Case study | stated repository and measured interval under published assumptions | universal savings or performance |

Negative and null results remain first-class evidence. A failed or inconclusive
attempt is retained with its scope; a later PASS does not reinterpret it.

## Programme architecture

The programme follows the public roadmap but orders delivery by trust dependency:

1. conversion and truthful proof surface;
2. independent verifier boundary;
3. immutable multi-platform verifier distribution;
4. slim trusted GitHub gate;
5. first-value and adoption commands;
6. transactional evidence publication;
7. cost intelligence;
8. native Linux end-to-end qualification;
9. optional A1 signed identity;
10. consented case studies and stable release.

Each milestone receives its own bounded implementation plan and branch. The
master plan defines dependencies and exit gates; a tranche plan fixes exact
files, interfaces, tests, and commits against the live base immediately before
implementation. This avoids inventing stale code signatures months in advance.

## Evolution ledger

| Milestone | Delivered result | Evidence | Residual boundary |
|---|---|---|---|
| Reliability foundation | managed-cache pins and terminal resource release merged | PR #68 and PR #69 | installed global producer remains older |
| Dry-run boundary | planning/replay distinction documented and contract-tested | PR #71, head `f3fb14a...`, receipt gate SUCCESS, merge `820a7fa...` | documentation does not add runtime behavior |
| Marketing audit | live GitHub, README, release, community, traffic, and competitor surfaces reviewed | 2026-08-29 read-only audit | no repository or setting mutation |

## Ordered milestones

### M0 — Durable programme control

**Outcome**

- Canonical specification, master plan, persistent goal, isolated branch, and restart procedure exist.

**Dependencies**

- `origin/main` exact anchor verified and divergent primary checkout preserved.

**Exit evidence**

- Markdown self-review passes; files committed on the isolated branch; clean status recorded.

**Impact**

- Long work can resume after compaction or restart without redefining scope.

**Residual risk**

- No product or public GitHub behavior changes.

### M1 — Conversion surface and dogfooding proof

**Outcome**

- A maintainer understands pain, mechanism, proof, fit, limits, and next action from the README.
- PR #71 becomes a compact public dogfooding case study.
- Advanced admission/resource detail no longer interrupts the new-user path.
- Social-preview render, metadata proposal, support surface, and community labels/forms are accurate.

**Dependencies**

- M0 complete; current issue forms and repository metadata re-audited live.

**Exit evidence**

- Focused repository-hygiene, release-contract, metadata, and link tests pass.
- Rendered preview is 1280x640, under GitHub's size limit, visually inspected.
- No claim exceeds the public receipt/release evidence.

**Impact**

- Improves comprehension and conversion without runtime changes.

**Residual risk**

- GitHub settings and organic adoption remain unproven until separately changed and observed.

### M2 — Physically independent verifier

**Outcome**

- Receipt/policy/schema/canonicalization logic lives in a bounded core plus verifier binary that has no runner, Docker, cache, admission, process, or resource dependency.

**Dependencies**

- M1 merged; exact receipt fixtures and compatibility contract frozen.

**Exit evidence**

- Golden receipt/policy bytes remain compatible; dependency graph proves verifier isolation;
  full deterministic suite and exact-head qualification pass.

**Impact**

- Creates a smaller trust root suitable for immutable distribution and GitHub verification.

**Residual risk**

- Artifact availability and identity are not yet established.

### M3 — Multi-platform immutable distribution

**Outcome**

- Exact-tag artifacts, installers, checksums, SBOM, provenance/attestation workflow,
  package-manager metadata, and an availability/smoke/qualification matrix exist for prioritized targets.

**Dependencies**

- M2 verifier boundary; approved distribution ADR; native builders for each claimed target.

**Exit evidence**

- Reproducible packaging tests, native install/uninstall smoke receipts, checksum/SBOM parity,
  rollback rehearsal, and exact artifact digest ledger.

**Impact**

- Removes source compilation as the default evaluation barrier.

**Residual risk**

- Package or release publication remains a separate owner action; smoke is not E2E.

### M4 — Slim trusted GitHub gate

**Outcome**

- The receipt gate downloads or restores one immutable, trusted verifier artifact and does not compile Rust, run Docker, access a package registry, or execute PR code.

**Dependencies**

- M3 immutable verifier artifact with rollback anchor.

**Exit evidence**

- PASS, stale, mismatched, oversized, malformed, missing, and untrusted fixtures fail or pass exactly as policy specifies; hosted wall time and network surfaces are recorded.

**Impact**

- Makes the remote control plane small, explainable, and cheap.

**Residual risk**

- Gate success remains A0 unless a later policy requires A1.

### M5 — First-value and adoption commands

**Outcome**

- `check`, project `init`, Actions `adopt`, `setup-github`, and Rust/Python/Node clean-room demonstrations provide short, deterministic activation paths.

**Dependencies**

- M3 installable artifacts; M4 stable gate contract; current config/migration behavior preserved.

**Exit evidence**

- Dirty-tree check produces no receipt; init/adopt/setup are idempotent and diff-first;
  unsupported workflow features remain blocked; three public fixtures reach their documented terminal state.

**Impact**

- Reduces time to first visible value without weakening evidence boundaries.

**Residual risk**

- Manual evidence publication remains until M6.

### M6 — Transactional evidence publication and pre-push

**Outcome**

- `publish`, `run --publish`, and reversible pre-push integration manage exact-SHA evidence refs without force, data loss, or hidden mutation.

**Dependencies**

- M5 activation surfaces and current verifier/gate contracts.

**Exit evidence**

- Local bare-remote concurrency, retry, stale receipt, network failure, conflicting ref,
  interrupted transaction, hook coexistence, uninstall, and rollback tests pass;
  one authorized live pilot succeeds exact-head.

**Impact**

- Removes manual worktree/ref choreography from the standard adoption path.

**Residual risk**

- Remote credentials and repository rules remain operator-controlled.

### M7 — Cost intelligence

**Outcome**

- Local-only CSV analysis and explicit estimates compare retained remote work,
  runner rates, rounding, included quota, gate cost, local cost, and break-even.

**Dependencies**

- Stable workflow vocabulary from M5/M6; dated official pricing source.

**Exit evidence**

- Golden fixtures cover zero-savings, under-quota, mixed-platform, malformed,
  rounding, and large inputs; outputs label measured, supplied, and inferred values.

**Impact**

- Lets users evaluate value without universal or fabricated savings claims.

**Residual risk**

- Estimates are not billing statements and pricing can drift.

### M8 — Native Linux x86_64 end-to-end qualification

**Outcome**

- Ubuntu LTS plus Docker Engine completes genuine Rust/Python/Node runner,
  receipt, verifier, publication, and slim-gate paths including failure modes.

**Dependencies**

- M3-M6 complete; authorized native Linux host and exact images available.

**Exit evidence**

- Native PASS/FAIL/timeout/cancellation/cleanup/cache/receipt/publication receipts
  retained with exact commits, binaries, image digests, host class, and policies.

**Impact**

- Promotes Linux from pending only on genuine native evidence.

**Residual risk**

- Windows complete run and other architectures remain separately scoped.

### M9 — Optional A1 signed identity

**Outcome**

- A standards-aligned detached signing envelope, signer policy, expiry/revocation,
  timestamp semantics, and offline failure modes extend A0 without breaking it.

**Dependencies**

- M2 stable core/verifier; approved signing and key-custody ADR; separate credential authority.

**Exit evidence**

- Valid, unknown, expired, revoked, tampered, offline, and unsigned-policy fixtures;
  no credential or public key is created or published without authorization.

**Impact**

- Allows repositories to require an accepted declarant identity.

**Residual risk**

- A1 still does not prove truthful execution or a managed/attested device.

### M10 — External proof and truthful stable release

**Outcome**

- Three consented public or anonymized case studies, launch materials, complete
  support matrix, migration/rollback guidance, and `v0.1.0` stable release exist.

**Dependencies**

- M1-M9 applicable exit gates; real repositories and explicit publication consent.

**Exit evidence**

- Reproducible case-study inputs and receipts; artifact matrix matches qualification;
  release checklist, fresh exact-head CCP qualification, hosted gates, install/rollback
  smokes, tag, release assets, checksums, attestations, and post-publication verification.

**Impact**

- CCP becomes responsibly promotable to external maintainers.

**Residual risk**

- Future pricing, platforms, identities, and repository-specific outcomes remain variable.

## Delegation and cost policy

1. Use deterministic tools before LLMs.
2. Delegate read-heavy inventory, documentation drafts, mechanical isolated edits,
   deterministic test execution, and bounded review to GPT-5.6 Luna.
3. Give each worker one owned file group and no overlapping writes.
4. Keep architecture, trust boundaries, security, cryptography, release qualification,
   external publication, integration review, and merge decisions with the primary agent.
5. Stop a cheap attempt after one clear failure and one focused correction.
6. Verify every delegated change centrally from the diff and relevant tests.

## Validation and publication gates

### Per commit

- Focused tests for the changed contract.
- `cargo fmt --all -- --check` when Rust or formatted Rust fixtures change.
- `CHANGELOG.md` updated for user-visible behavior.

### Per pull request

- Full deterministic suite: `cargo test --locked --workspace --all-targets --all-features`.
- Clippy: `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`.
- Documentation: `cargo doc --locked --workspace --no-deps` and release metadata parity.
- Exact-head code review and no unresolved material findings.
- CCP receipt qualification only with a fresh exact authorization envelope.

### External mutations

- Non-forced push, PR creation, ready transition, evidence publication, merge,
  repository metadata, label/milestone edits, social-preview upload, release,
  package publication, signing, and case-study publication are separately reported
  and gated by live exact state.

## Interruption and recovery

Before restart or handoff:

1. record worktree, branch, HEAD, base, dirty files, and remote state;
2. stop or record active workers and processes;
3. record terminal checks and explicitly unproven gates;
4. commit recoverable in-scope work when authorized;
5. preserve any receipt, artifact digest ledger, and rollback anchor;
6. write the exact next command and reread this specification before resuming.

No temporary path alone counts as durable preservation. The branch commit in the
main Git object store is the minimum local checkpoint; remote publication remains
a separate gate.

## Milestone report format

- Result obtained
- Terminal validation evidence
- Claims or behavior changed
- Residual risks and explicit non-claims
- Exact next dependency and required authority

## Completion checklist

- [ ] M0 programme control is committed and recoverable.
- [ ] M1 new-user conversion surface is clear, tested, and live on GitHub.
- [ ] M2 verifier is physically independent and fixture-compatible.
- [ ] M3 prioritized artifacts are available, smoke-verified, and provenance-bound.
- [ ] M4 slim gate verifies with an immutable trusted artifact.
- [ ] M5 new users reach first value through documented short paths.
- [ ] M6 evidence publication and pre-push are transactional and reversible.
- [ ] M7 cost reports preserve assumptions and evidence classes.
- [ ] M8 native Linux complete run is genuinely qualified.
- [ ] M9 A1 identity is standards-aligned and correctly bounded, or explicitly deferred from stable scope by an approved revision.
- [ ] M10 three consented case studies and the stable release are published and verified.
- [ ] Every public claim maps to exact current evidence.
- [ ] Every release byte maps to an exact tag, checksum, SBOM, and provenance record.
- [ ] Installation, smoke, E2E qualification, and identity assurance remain visibly distinct.
- [ ] Documentation, examples, support matrix, and rollback guidance match current behavior.
- [ ] No required human, native-platform, consent, security, or publication gate is represented as complete without terminal evidence.

Completion is unproven until every applicable item has authoritative current evidence.
