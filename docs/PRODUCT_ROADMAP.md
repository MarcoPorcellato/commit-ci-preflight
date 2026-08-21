# Proof-carrying CI product roadmap

## Document control

| Field | Value |
|---|---|
| Status | Approved for staged implementation; completion requires evidence per tranche |
| Product | Commit CI Preflight |
| Category | Proof-carrying CI / CI receipts for exact Git commits |
| Baseline | `origin/main` at `17737e002a079124d9ce1cb458bd64ab229aa9d8` |
| Date | 2026-08-13 |
| Release objective | Truthful `v0.1.0` stable after distribution, Linux qualification, and external evidence |

This roadmap turns the technically mature beta into a product that a new user
can install, understand, adopt, and evaluate economically. It supersedes no
implemented receipt, policy, or security contract. The historical execution
record remains in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md).

## Product thesis

> GitHub must verify CI, not necessarily recompute it.

Commit CI Preflight runs deterministic heavy checks on trusted local hardware,
emits a canonical receipt bound to the exact Git commit, and leaves GitHub a
small control-plane gate that validates the receipt and publishes status on the
exact pull-request head for the exact repository state.

The durable value proposition is avoiding duplicate remote computation. It is
not based on a claim that GitHub charges for self-hosted runners. GitHub
postponed the announced self-hosted platform charge; current hosted-runner
prices and included quotas are policy variables and may change independently.

## Product boundaries

Commit CI Preflight moves only covered, deterministic work. These controls stay
remote or native when applicable:

- code review, branch rules, permissions, merge queues, and protected environments;
- trusted secrets, signing, release publication, and deployment;
- native behavior not genuinely executed on an accepted platform;
- hostile-contributor execution requiring a stronger trust boundary;
- infrastructure integrations not represented by the explicit local plan.

The current receipt provides **A0 integrity and repository-policy assurance**.
It proves that the receipt is internally consistent, untampered, exact-commit
bound, and policy-conforming. It does not prove producer identity or truthful
execution against a malicious insider.

Future assurance levels are explicit and cumulative:

| Level | Claim |
|---|---|
| A0 — Integrity | Canonical receipt integrity and repository-policy match |
| A1 — Signed identity | A policy-accepted identity signed the receipt |
| A2 — Managed device | The signing key is bound to an approved managed device |
| A3 — Execution attestation | Evidence is bound to an independently attested execution environment |

Signatures identify a declarant; they do not by themselves prove truthful
execution. No documentation or UI may collapse these levels into one “trusted”
badge.

## Success metrics

### Activation

- A user with a Rust, Python, or Node repository reaches a first local PASS by
  following one page and running at most three primary commands.
- No manual worktree, receipt copy, or evidence-branch command is required for
  the standard path.
- Installers are available for five prioritized targets, while availability
  remains clearly separated from end-to-end qualification.

### Remote cost and latency

- The GitHub receipt gate does not compile Rust, access a package registry, run
  Docker, or execute pull-request code.
- Target gate wall time is below 15 seconds after runner allocation.
- Cost reports distinguish list-price, included quota, local electricity or
  hardware assumptions, and theoretical versus billed savings.

### Quality and trust

- Existing receipt and policy fixtures remain byte-compatible through the
  verifier split.
- Every new release artifact has checksums, SBOM, provenance evidence, and a
  documented qualification state.
- Linux x86_64 and macOS arm64 complete runner–receipt–verifier–gate paths have
  genuine receipts before stable release.
- No benchmark or case study uses fabricated timing, cost, platform, identity,
  or adoption evidence.

## Architecture direction

The repository evolves into a Rust workspace only through behavior-preserving
slices:

```text
crates/
  ccp-core/       receipt, policy, canonical JSON, schema, digest, common types
  ccp-verifier/   bounded parsing, policy evaluation, reports, verifier binary
  ccp-runner/     runtime, process, workspace, cache, admission, resource guard
  ccp-github/     evidence publication, workflow generation, cost import
  ccp-cli/        user-facing command composition
```

Dependency direction is inward. `ccp-verifier` must not depend on Docker,
process supervision, cache management, admission, or host resource code. The
first split may retain compatibility re-exports so downstream source and golden
fixtures do not change in the same tranche.

Admission, watchdog, and resource protection remain supported as agent-safe
local execution infrastructure, but receive no new features until the product
activation and distribution gates below are complete.

Owner-authorized safety exception (2026-08-13): observation-only local resource
history may proceed before those gates because it changes no admission or
watchdog decision and supplies the evidence needed to evaluate a future policy
without guessing. [RESOURCE_OBSERVATION_HISTORY.md](RESOURCE_OBSERVATION_HISTORY.md)
is the controlling scope, privacy and qualification contract. Any forecast or
threshold change remains blocked by its separate owner gate.

Owner-authorized safety exception (2026-08-21): the planned opt-in agent
admission lifecycle may proceed as a narrow orphan prevention safeguard. It
must preserve the single Rust coordinator.
It must fail closed on unknown ownership.
It must require an explicit claim by a live activity.
It must never execute a stored or hidden command.
It neither revives a terminated chat nor relaxes any release, receipt, or
GitHub verification gate.

Resource history v2 additionally provides bounded workload/executor context so
samples from different launchers are not mixed. Adoption remains cooperative:
each official launcher must use `guard exec`, and direct container-runtime
processes are explicitly outside coverage. The audited inventory and rollout
contract live in
[ORBSTACK_TELEMETRY_COVERAGE.md](ORBSTACK_TELEMETRY_COVERAGE.md).

## Delivery sequence

Each tranche is a separate pull request unless a smaller prerequisite is
needed. Later work may be stacked on a verified branch, but every PR remains
independently reviewable and reversible.

### PR 1 — Positioning and repository hygiene

Deliverables:

- publish a benefit-led proof-carrying CI hero and category definition in
  `README.md` that can be understood without architectural context;
- add explicit ideal/non-ideal user segmentation to the README onboarding path;
- keep competitive comparison bounded to official sources for:
  local runners, pipeline engines, caches, and self-hosted runners;
- add a compact, assumption-labeled cost illustration (with formulas and no
  universal savings claim);
- add a concise A0 trust/non-claim summary before deep architecture and
  component details;
- add a five-minute read-only first-inspection path using only documented,
  existing commands and examples; keep real execution in the isolated
  clean-room tutorial until one-command adoption exists;
- expand `CONTRIBUTING.md` with setup + component/test guidance for reviewable
  onboarding.

Expanded onboarding and PR 1 delivery criteria:

- publish updated repository roadmap and contributor guidance for exact issue
  triage, PR scope, review evidence, and component-level test discipline;
- close completed historical issues with exact implementation evidence linked;
- define `0.1.0 stable` milestone and keep current labels:
  `adoption`, `security`, `cost-model`, `platform`, `good first issue`;
- repository description aligned to proof-carrying CI;
- social-preview source asset and truthful owner instructions for setting it on
  GitHub without claiming the remote setting is already applied.

Exit gate:

- a new visitor can answer problem, differentiation, first trial, and limits
  from the README in one pass, including the first-inspection path and explicit
  non-claims;
- all public links and repository templates validate;
- no installation command claims an unavailable package or platform qualification;
- README claims are scoped to current facts and are marked when they are
  assumptions.

### PR 2 — Physically independent verifier

Deliverables:

- introduce `ccp-core` and `ccp-verifier` workspace crates;
- preserve receipt, policy, report, schema, and canonical-byte behavior;
- add a dedicated `ccp-verifier` binary with only `verify` and schema surfaces;
- prohibit runner/runtime dependencies from its resolved dependency graph;
- add equivalence, maximum-input, malformed-input, and binary-size receipts;
- produce a static `x86_64-unknown-linux-musl` verifier candidate locally or in
  an explicitly authorized release workflow.

Exit gate:

- old CLI and dedicated verifier return byte-equivalent reports and exit codes
  over all positive and negative fixtures;
- dependency audit proves no Docker/process/cache/resource modules are reachable
  from the verifier binary;
- no receipt v1 or policy v1 compatibility break.

### PR 3 — Slim trusted GitHub gate

Deliverables:

- use `ubuntu-slim` for the receipt gate;
- download a verifier pinned by immutable release identity and SHA-256;
- verify checksum and available provenance/signature before execution;
- remove Cargo compilation and registry access from the gate;
- retain separate trusted-policy, verifier, and bounded-evidence inputs;
- publish a concise summary: receipt ID, exact commit, platform, age, and checks;
- benchmark cold runner allocation separately from verifier execution.

Exit gate:

- no PR/evidence code execution, Docker, Cargo, cache restore, or secrets;
- verifier execution target below 15 seconds after allocation;
- missing, oversized, stale, digest-invalid, untrusted, or mismatched inputs fail closed;
- rollout includes a rollback workflow pinned to the last trusted verifier.

PR 3 is blocked until PR 4 has a trustworthy immutable verifier artifact, or a
minimal bootstrap release is separately authorized. It must never download a
moving `latest` asset.

### PR 4 — Multi-platform signed distribution

Deliverables:

- evaluate and pin a maintained Rust distribution tool through an ADR;
- binaries for Linux x86_64/arm64, macOS x86_64/arm64, and Windows x86_64;
- shell and PowerShell installers;
- Homebrew formula/tap path and crates.io metadata;
- `cargo-binstall`-compatible release metadata;
- checksums, SBOM, GitHub artifact attestations, and Sigstore verification path;
- installation matrix separating `AVAILABLE`, `SMOKE_VERIFIED`, and
  `END_TO_END_QUALIFIED`.

Exit gate:

- every published byte maps to an exact source tag and checksum;
- installation/uninstallation tests run natively where claimed;
- no package, registry, signature, or key is published without explicit release authorization;
- documentation never infers runtime qualification from binary availability.

### PR 5 — One-command adoption and development mode

Deliverables:

- `ccp check`: dirty-tree development execution with no attestable receipt;
- `ccp init`: detect Rust/Python/Node lockfiles and propose a bounded config;
- `ccp adopt --from-github-actions`: compose the inert migration report into a
  reviewed adoption proposal;
- `ccp setup-github`: generate pinned workflow and policy guidance;
- dry-run and diff-first defaults; no mutation without explicit confirmation;
- image-digest resolution is explicit and never silently selects a mutable tag.

Exit gate:

- public clean-room Rust, Python, and Node repositories reach first PASS from a
  one-page guide;
- generated files are deterministic and idempotent;
- unsupported Actions features remain manual/blocked, never guessed;
- `check` can never emit or publish A0 evidence.

### PR 6 — Evidence publication and pre-push workflow

Deliverables:

- `ccp publish` verifies receipt, policy, clean HEAD, remote identity, and exact SHA;
- hides isolated worktree/ref creation and cleanup behind transactional logic;
- handles exact existing branch, retry, network failure, and stale receipt safely;
- `ccp run --publish` composition;
- `ccp hooks install --pre-push` with reversible installation and no silent overwrite;
- evidence backend interface retaining SHA-derived branches as v1 default.

Exit gate:

- no standard-path manual Git worktree, receipt copy, or evidence-branch command;
- source push cannot be claimed protected unless evidence publication completed first;
- unique interrupted work is preserved, not deleted;
- branch/ref mutation tests use isolated local bare repositories before remote trials.

### PR 7 — Cost intelligence

Deliverables:

- `ccp cost analyze <github-usage.csv>` with bounded, local-only parsing;
- `ccp cost estimate` and `ccp cost compare` for explicit assumptions;
- SKU rates, per-job rounding, included quota, retained remote jobs, gate cost,
  local cost, gross/net savings, wait time, and break-even;
- canonical JSON plus human-readable report;
- pricing data version/date and no automatic billing or account access;
- privacy statement: reports remain local unless the user exports them.

Exit gate:

- golden fixtures cover zero-savings, under-quota, mixed-platform, malformed,
  rounding, and large-file cases;
- outputs separate measured, supplied, and inferred values;
- current prices are sourced from official GitHub documentation and can be
  updated without code changes;
- no universal savings claim.

### PR 8 — Linux x86_64 end-to-end qualification

Deliverables:

- native Ubuntu LTS plus Docker Engine qualification;
- Rust, Python, and Node cold/warm fixtures;
- PASS, FAIL, timeout, cancellation, descendant cleanup, cache replay, receipt,
  verifier, publication, and slim-gate evidence;
- bounded before/after benchmark on a public reproducible corpus;
- Linux support state promoted only from genuine native receipts.

Exit gate:

- complete path is proven on Linux x86_64 and macOS arm64;
- all receipts and timing metadata are retained with exact commits and tool versions;
- no macOS-hosted container run is labelled Linux-host-native.

### PR 9 — Signed identity assurance

Deliverables:

- ADR selecting optional signing envelope and interoperability path;
- A1 signer identity policy, allowlist, expiry/revocation handling, and timestamp semantics;
- detached bundle verification and stable fail-closed findings;
- evaluate DSSE/in-toto compatibility instead of inventing an incompatible envelope;
- unsigned A0 remains explicit and backwards compatible where policy permits it.

Exit gate:

- signature verifies identity of the declarant without claiming execution truth;
- keyless, key-backed, and offline/revoked cases are tested as applicable;
- no signing credential is created or published without separate owner authorization;
- threat model and assurance UI distinguish A0–A3.

### PR 10 — External case studies and stable release

Deliverables:

- three consented real repositories: small, medium/monorepo, and agent-driven;
- before/after Actions minutes, billed/list cost, local runtime, gate runtime,
  feedback latency, and retained remote checks;
- anonymization and publication consent for any non-public data;
- stable-release checklist, migration/rollback notes, and launch article;
- `v0.1.0` only after all required evidence is complete.

Exit gate:

- no fabricated adoption, timing, billing, or “used by” claim;
- case-study methodology and raw public fixtures are reproducible;
- release artifacts and supported platforms match the qualification matrix;
- unresolved P0/P1 security or compatibility findings are zero.

## Cross-cutting contracts

### Reproducibility language

Use **reproducible execution contract**, not absolute hermeticity, when network
or mutable dependency caches can affect execution. A future two-phase design is:

```text
ccp prepare   # network may populate declared caches; no trusted receipt
ccp attest    # network disabled; only this phase emits publishable evidence
```

This design requires its own ADR and is not implied by receipt v1.

### Evidence storage

The v1 SHA-derived evidence branch stays until `ccp publish` makes it invisible
to normal users. A later backend may use an append-only branch, custom refs,
OCI artifacts, or a GitHub App. Storage changes must preserve exact-commit
lookup, fail-closed retrieval, retention policy, and rollback.

### Compatibility

- Receipt, policy, and report schemas are independently versioned.
- Unknown fields and versions fail closed.
- Workspace refactors do not justify a schema bump.
- The legacy `commit-ci-preflight` binary remains available through the stable
  release unless a documented migration path exists.
- The short `ccp` name remains a technical alias; public messaging uses the
  full product name and proof-carrying CI category.

### Dependency and supply-chain policy

Every new dependency or distribution tool requires:

- purpose and rejected alternatives;
- license and transitive-license review;
- maintenance/security posture;
- minimal feature set and lockfile review;
- runtime-network and build-script analysis;
- SBOM and third-party notice update.

## Repository and community programme

Immediate repository hygiene:

- close completed historical issues with exact merge evidence;
- milestone `0.1.0 stable` with the remaining product tranches;
- labels: `adoption`, `security`, `cost-model`, `platform`, `good first issue`;
- bug, feature, and adoption-report issue forms;
- PR template with trust claims and evidence checklist;
- Discussions enabled for design partners and adoption support;
- accurate description, topics, social-preview asset, and roadmap links;
- “Used by” section only after verifiable external use exists.

## Verification ladder for every PR

1. exact live base, clean isolated worktree, and file allowlist;
2. focused unit/contract tests for changed behavior;
3. format, Clippy, documentation, and full Rust suite;
4. generated schema/metadata drift checks;
5. diff/check and dependency/license review;
6. complete local CCP run on the exact clean commit;
7. independent receipt verification;
8. exact-SHA evidence publication and remote receipt gate;
9. merge only with fresh terminal checks and head-pinned merge;
10. post-merge proof that `origin/main` contains the intended commit.

Native, signed, release, billing, and external-adoption claims require their
own direct evidence and cannot be substituted by the generic ladder.

## Stop conditions and owner gates

Implementation stops for explicit owner authority before:

- publishing to crates.io, Homebrew, Winget/Scoop, or another registry;
- creating or using signing identities, KMS keys, or release secrets;
- enabling paid services or workflows with material cost;
- changing repository visibility, ownership, collaborators, or branch security;
- publishing private billing exports or non-public case-study data;
- claiming Windows/Linux/macOS native qualification without native execution;
- creating `v0.1.0` stable or a public launch announcement.

Missing hardware, credentials, or design partners are recorded as `PENDING`,
not converted into source-only PASS.

## Programme completion definition

This roadmap is complete only when:

- PR 1–PR 9 are merged with their exit evidence;
- Linux x86_64 and macOS arm64 complete paths are qualified;
- install, init/check/run/publish, slim gate, and cost report work end to end;
- A0/A1 assurance is implemented and truthfully documented;
- three consented case studies exist or the stable-release decision explicitly
  removes that requirement through a reviewed roadmap amendment;
- `v0.1.0` stable artifacts are published and independently verified;
- the final requirement-by-requirement audit has no missing evidence.
