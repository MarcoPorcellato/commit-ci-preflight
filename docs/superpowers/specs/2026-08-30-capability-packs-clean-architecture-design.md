---
type: execution-specification
title: "Clean Architecture and Advanced Linux Capability Packs"
description: "Evolve CCP into a Mac-first local Linux qualification platform without coupling its evidence core to individual analysis tools."
status: approved-direction
last_verified: 2026-08-30
---

# Clean Architecture and Advanced Linux Capability Packs

This file is the canonical execution contract for the current programme.
Conversation history, research notes, and issues are supporting context, not
competing plans.

## Outcome

Commit CI Preflight remains a small, language-agnostic qualification and
evidence engine while becoming substantially easier to use for advanced Linux
checks from an Apple Silicon Mac.

The programme delivers:

1. an internal Clean Architecture refactor with no observable compatibility
   change;
2. a versioned, inspectable Capability Pack contract outside the evidence
   domain;
3. one dogfooded Rust deep-analysis reference pack;
4. one language-agnostic repository-security reference pack;
5. a separately versioned design for normalized machine-readable findings;
6. a stable release checkpoint before additional product expansion.

The result is falsified if CCP becomes a new build language, package manager,
GitHub Actions emulator, cloud runner service, or tool-specific monolith.

## Authoritative anchors

| Item | Verified state | Evidence |
|---|---|---|
| Repository | `MarcoPorcellato/commit-ci-preflight` | local `origin` remote |
| Delivery worktree | `/Users/marco1/Documents/CODICE con VS CODE/ccp-worktrees/capability-packs-clean-architecture-v1` | `git worktree list --porcelain` |
| Base revision | `5fed7c443504969e62980141048f9279f9fa1dfe` | fetched `origin/main`, 2026-08-30 |
| Delivery branch | `codex/capability-packs-clean-architecture-v1` | live branch |
| Compatibility envelope | strict preservation | owner approval, 2026-08-30 |
| Product direction | Capability Packs plus later generic evidence adapters | owner approval, 2026-08-30 |
| Current milestone | M0 specification and implementation plan | this document |

Reverify drift-prone anchors before every external mutation, qualification, or
release operation.

## Status vocabulary

- **Verified:** terminal authoritative evidence exists for the exact revision.
- **In delivery:** recoverable committed work exists but its milestone gate is
  incomplete.
- **Blocked:** a named external, technical, or human dependency prevents the
  path.
- **Planned:** dependency-ordered work with an explicit exit gate.
- **Deferred:** deliberately excluded until a predecessor justifies it.
- **Qualified:** the exact candidate passed every platform and evidence gate
  claimed for it.

## Product decision

CCP's durable moat is not executing more commands than existing CI systems. It
is the combination of:

- host resource admission and coordination;
- an exact clean source binding;
- explicit shell-free execution in pinned Linux runtimes;
- bounded caches, artifacts, timeouts, and cleanup;
- canonical receipts and independent policy verification.

Advanced tools therefore integrate through declarative Capability Packs. The
domain core must not contain branches for Semgrep, Trivy, Miri, Valgrind,
CodeQL, or any other individual analyzer.

### Rejected alternatives

1. **Tool-specific core integrations.** Rejected because licensing, release
   cadence, report formats, image support, and databases would force external
   volatility into the evidence domain.
2. **Executor-only documentation.** Rejected because hand-written argv and
   image wiring do not provide a compelling or safe onboarding experience.
3. **A new workflow or build DSL.** Rejected because act, Dagger, Bazel, Nix,
   Task, Just, Make, and pre-commit already own those concerns.

## Clean Architecture boundaries

Dependency direction is inward:

```text
CLI / GitHub / pack tooling
          |
Application use cases
plan / inspect / execute / verify
          |
Domain
check plan / policy / evidence / receipt
          ^
Ports
runtime / storage / process / clock / admission
          ^
Adapters
Docker-compatible runtime / filesystem / GitHub / capability packs
```

### Domain

The domain owns canonical check plans, policy decisions, evidence status,
receipt identity, and compatibility rules. It contains no Clap parsing,
filesystem discovery, Docker argv construction, GitHub API behavior, analyzer
catalog, or human output formatting.

### Application

Application services express bounded use cases such as plan, inspect, execute,
and verify. They receive request objects and ports rather than accumulating
telescoping function parameters.

### Ports

Ports describe effects required by the application layer: runtime execution,
process supervision, storage, filesystem operations, time, admission, and
publication boundaries. Test doubles implement the same narrow contracts.

### Adapters

Docker-compatible execution, local filesystems, the CLI, GitHub integration,
and Capability Pack loading are replaceable adapters. They may depend inward;
the domain never depends outward on them.

### Compatibility facade

Existing public Rust modules and functions remain available. Internal request
objects may replace parameter chains, but existing public functions delegate to
the new use cases. This preserves downstream compilation while allowing the
implementation to become coherent.

## Capability Pack contract

A pack is inert, declarative input. Installing or inspecting a pack never runs
project code. Execution remains an ordinary reviewed CCP plan.

Every pack records:

- stable pack ID, schema version, pack version, license, and upstream sources;
- supported host/runtime architectures and required runtime features;
- pinned OCI image references and tool versions;
- explicit shell-free check argv, working directories, dependencies, resource
  profiles, and timeouts;
- declared caches and bounded output artifacts;
- rule, type-stub, corpus, advisory, or vulnerability-database provenance and
  digest where applicable;
- network policy and offline preparation requirements;
- deterministic, schedule-sensitive, or bounded-nondeterministic evidence
  classification;
- documented PASS/FAIL semantics and known blind spots.

Packs expand into existing configuration primitives. They do not silently
override repository policy, broaden network access, install host software,
pull mutable images, execute a shell, or publish evidence.

### Trust model

- A pack is untrusted data until its schema, bounds, source, and digests pass.
- A pack version is immutable.
- Tool and database freshness is distinct from integrity.
- A report digest proves exact bytes, not the truth or completeness of a tool.
- Findings remain local unless a separately authorized publication surface is
  designed and reviewed.
- CodeQL is not an official default pack until licensing, private-repository
  eligibility, Linux ABI, and Apple Silicon execution are explicitly resolved.

## Reference packs

### `rust-deep`

The first pack is dogfooded on CCP. Its stable-release minimum is limited to
four independently selectable profiles: strict Clippy, cargo-deny, bounded
Miri, and one Linux AddressSanitizer fixture. Each profile may be omitted for a
consumer repository that lacks its explicit prerequisites; omission is
reported and is never a PASS.

Loom, cargo-fuzz corpus replay, open-ended fuzzing, mutation testing, other
sanitizers, and reproducible-build comparison are deferred experiments. They
do not belong to the first stable pack exit gate.

Open-ended fuzzing, timing-sensitive concurrency claims, and mutation testing
must not be represented as ordinary deterministic checks. Every such profile
records seed, budget, corpus digest, schedule bounds, timeout, architecture,
and residual uncertainty.

### `secure-repository`

The second pack targets common repository and supply-chain risks. Its
stable-release minimum is limited to actionlint, zizmor, Trivy filesystem or
configuration scanning, and OSV-Scanner lockfile scanning.

Syft/Grype, ShellCheck, Hadolint, and Semgrep Community Edition with reviewed
local rules remain deferred candidates until the minimum pack is qualified.

Vulnerability or advisory results bind the exact database snapshot and its
freshness metadata. SBOM generation is evidence inventory, not a vulnerability
PASS. Workflow linting is not proof that GitHub will execute the workflow
identically.

## Findings and report boundary

Current receipts prove command identity, status, output digest, and bounded
artifact evidence. They do not interpret analyzer findings.

The first Capability Pack release uses those existing guarantees and a local
bounded report validator external to the receipt schema. A future findings
contract may normalize SARIF or versioned JSON only after the packs establish
real requirements.

Any future receipt extension must:

- use a new explicit schema version;
- retain verification of all historical receipts;
- bind the raw report digest and parser identity;
- impose byte, record, nesting, string, and path limits;
- distinguish tool execution failure from policy findings;
- preserve rule/database provenance and freshness separately;
- avoid raw source, secrets, local paths, and unbounded messages;
- support independent verification without executing the analyzer.

## Scope

- Record and progressively enforce the Clean Architecture boundaries.
- Reduce `main.rs` dispatch responsibility and `run.rs` parameter telescoping
  through compatibility-preserving seams.
- Define and validate an inert Capability Pack manifest.
- Provide deterministic expansion or generation into existing CCP contracts.
- Deliver and dogfood `rust-deep`.
- Deliver `secure-repository` after the first pack proves the contract.
- Document tool licenses, architectures, offline/freshness behavior, evidence
  class, resource cost, and limitations.
- Preserve a durable checkpoint and milestone ledger throughout delivery.

## Non-goals

- Reimplement GitHub Actions semantics or marketplace actions.
- Create a new task language, build graph, package manager, container runtime,
  hypervisor, remote cache, hosted runner fleet, IDE, or cloud dashboard.
- Automatically install tools on macOS.
- Automatically grant Docker socket, privileged mode, host networking, secrets,
  or mutable network access.
- Claim native Linux hardware equivalence for emulation or a Mac-hosted VM.
- Claim monetary savings without repository-specific billing evidence.
- Implement a new receipt schema before pack evidence demonstrates the need.

## Invariants and boundaries

- Existing CLI syntax, exit codes, valid configuration bytes and digests,
  receipt bytes and IDs, policy schemas, JSON shapes, and public Rust facade
  remain compatible.
- M0 records a compatibility corpus under
  `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/compatibility/`.
  Its manifest names and hashes the current CLI help surfaces, exit-code
  cases, canonical v1/v2/matrix plan fixtures, privacy-normalized dry-run
  fixtures, receipt and policy fixtures, verification decisions, and a
  downstream compile fixture that imports the supported public Rust facade.
  Dry-run normalization replaces only explicit workspace host-path fields and
  the `src=` segment of CCP-generated `type=bind,src=...,dst=...` argv with
  typed tokens. It preserves JSON structure, every non-mount argv byte,
  container destinations, access, purpose, and logical IDs. M1 must compare the
  candidate against these files and produce no unexplained byte,
  normalized-structure, or compile delta.
- No analyzer-specific dependency enters the evidence domain.
- All new parsing is bounded, deny-unknown where appropriate, deterministic,
  and fail closed.
- Source remains read-only during qualification; writable outputs are explicit.
- Network is disabled by default. Preparation and offline execution are
  distinct operations and evidence classes.
- The public repository uses hosted CI by default under the economic policy.
  CCP heavy work requires an explicit non-economic exception and exact
  authorization.
- Commit, push, PR creation, ready transition, merge, tag, release, package
  publication, stable installation, and external evidence publication remain
  separate gates.
- Architecture, security, integration, release, and final evidence decisions
  stay with the primary agent.

## Ordered milestones

### M0 — Durable design and TDD execution plan

**Outcome**

- Canonical specification, persistent goal, gap inventory, and dependency-
  ordered TDD plan are committed locally.
- A named and hash-manifested compatibility corpus makes the strict envelope
  executable rather than aspirational.

**Dependencies**

- Approved product direction and exact isolated base.

**Exit evidence**

- Markdown contract checks and independent documentation review.
- Compatibility inventory with complete file hashes and commands that can
  regenerate or compare every item without network or project execution.
- Local commit on the delivery branch.

**Impact**

- The programme can survive compaction, restart, or delegation.

**Residual risk**

- No runtime behavior or product capability has changed.

### M1 — Compatibility-preserving application seam

**Outcome**

- One vertical execution path uses a coherent request/dependency object and
  narrow application service while existing public entry points delegate to it.
- CLI parsing/formatting is separated from the selected use case.

**Dependencies**

- M0.

**Exit evidence**

- RED/GREEN unit and integration tests.
- Byte-for-byte comparison against the M0 CLI, exit-code, plan, receipt,
  policy, verification, and JSON corpus, plus exact comparison of the
  privacy-normalized dry-run projections.
- Compile success of the M0 downstream public-facade fixture without source
  changes.
- Hosted CI terminal on the exact commit before integration.

**Impact**

- Establishes an architectural seam without a broad rewrite.

**Residual risk**

- Other command families remain on their current internal structure.

### M2 — Capability Pack schema and inert inspection

**Outcome**

- A bounded versioned manifest, validator, canonical representation, and inert
  inspection/expansion surface exist without executing project code.

**Dependencies**

- M1 seam or an independently reviewed adapter boundary.

**Exit evidence**

- Parser abuse/boundary tests, golden canonicalization fixtures, unknown-field
  rejection, path/image/license/provenance validation, and no-execution tests.
- Compatibility suite remains byte-stable.

**Impact**

- Tool integrations become data-driven rather than core branches.

**Residual risk**

- No reference pack is yet qualified.

### M3 — `rust-deep` reference pack

**Outcome**

- A documented subset of Rust deep checks runs through ordinary CCP primitives
  in a Docker-compatible Linux VM hosted on Apple Silicon macOS and produces
  bounded local artifacts.

**Dependencies**

- M2 and reviewed tool/image/license matrix.

**Exit evidence**

- Synthetic positive and negative fixtures.
- The four stable profiles are inspected and fixture-qualified; strict Clippy,
  cargo-deny, and bounded Miri are dogfooded where their declared prerequisites
  exist, and the AddressSanitizer profile executes a dedicated Linux fixture.
- Every omitted profile is recorded as NOT_RUN with its missing prerequisite.
- Deferred or nondeterministic profiles cannot satisfy this milestone.
- Exact architecture, image, toolchain, corpus/rules, and artifact hashes.

**Impact**

- Demonstrates high-value Linux analysis unavailable or awkward in ordinary
  macOS-only workflows.

**Residual risk**

- Proves only the exact Mac host, Docker-compatible runtime, Linux guest
  architecture (initially `linux/arm64`), image, and selected tools. It does not
  prove a native Linux host, `linux/amd64`, or macOS-native execution.

### M4 — `secure-repository` reference pack

**Outcome**

- Repository, workflow, dependency, SBOM, vulnerability, and rule-based checks
  are available through reviewed bounded profiles.

**Dependencies**

- M2 and lessons from M3.

**Exit evidence**

- For each of the four stable tools: exact version, upstream source commit or
  release, authoritative URL, license and redistribution mode, OCI image
  digest, `linux/arm64` support evidence, rules or database snapshot digest and
  freshness metadata where applicable.
- DB/rules provenance fixtures, stale/offline failure tests, and
  positive/negative repositories.
- Deferred candidates cannot satisfy this milestone.

**Impact**

- Extends CCP beyond language-specific tests without polluting its core.

**Residual risk**

- Scanner blind spots and database freshness remain explicit.

### M5 — Generic findings contract decision

**Outcome**

- Evidence from M3/M4 produces either an approved versioned report design or a
  documented decision that artifact digests remain sufficient.
- The decision is recorded in
  `docs/superpowers/specs/2026-08-30-findings-contract-decision.md`, owned by
  the primary agent, with exactly one terminal verdict: `GO_VERSIONED_SCHEMA`
  or `NO_GO_KEEP_ARTIFACT_DIGESTS`.

**Dependencies**

- Real report formats and policy needs observed in M3/M4.

**Exit evidence**

- Threat model, parser limits, compatibility analysis, independent verifier
  impact, and migration plan.
- A per-tool table covering report format/version stability, maximum observed
  bytes and records, policy semantics, parser ownership, privacy fields, and
  provenance needs.
- Every decision checklist item is resolved; missing evidence forces
  `NO_GO_KEEP_ARTIFACT_DIGESTS` rather than an ambiguous defer.

**Impact**

- Prevents speculative receipt/schema complexity.

**Residual risk**

- Implementation, if approved, is a separately gated programme.

### M6 — Stable release checkpoint

**Outcome**

- The compatible architectural seam, pack contract, qualified reference packs,
  installation guidance, changelog, release notes, and rollback procedure are
  ready as one stable candidate.

**Dependencies**

- M1-M4 terminal; M5 decision terminal.

**Exit evidence**

- Exact-head hosted CI and platform matrix appropriate to the claims.
- Release artifact hashes, SBOM, independent verifier result, install/rollback
  proof, and owner-authorized tag/release publication.

**Impact**

- Establishes a stable usable version before additional functionality.

**Residual risk**

- Unqualified platforms and optional tools remain explicitly unsupported.

## Delegation and cost policy

1. Use deterministic tools before LLMs.
2. Delegate research, inventory, documentation drafts, mechanical isolated
   edits, test execution, and bounded reviews to Luna or Spark.
3. Assign one owner per file group; no overlapping writes.
4. Keep architecture, security, integration, release, and final judgment with
   the primary agent.
5. Run narrow deterministic checks before broader validation.
6. Stop a cheap worker after one clear failure and one focused correction.

## Validation and publication gates

- Every production change follows RED/GREEN TDD.
- Every milestone includes focused tests and compatibility fixtures.
- Full hosted CI is the authoritative public-repository integration gate.
- A CCP heavy run is not the default for this public repository and requires a
  separately documented non-economic exception.
- No generated pack may execute during inspection or configuration generation.
- Tool licenses and distribution rights are verified before an official pack
  embeds or redistributes anything.
- Push requires the exact local head and non-force destination.
- PR creation, ready transition, merge, tag, release, package publication, and
  stable installation are separate exact-state gates.

## Interruption and recovery

Maintain `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/`
with a concise persistent goal, current progress, exact branch/HEAD/base,
completed validations, active worker state, unproven gates, and next action.

Before a restart or handoff:

1. stop or record active workers and processes;
2. record worktree, branch, exact HEAD, base, dirty state, and remotes;
3. save in-scope work in a recoverable local commit;
4. record local, hosted, and release evidence separately;
5. preserve exact resume commands and authorization boundaries.

## Milestone report format

- Result obtained
- Terminal validation evidence
- Behavior or claims changed
- Residual risks
- Next dependency and approval gate

## Completion checklist

- [ ] M0 canonical specification, persistent goal, and TDD plan are committed.
- [ ] M1 creates a tested compatibility-preserving architectural seam.
- [ ] Existing CLI, exit codes, plans, receipts, schemas, and public facade are
      proven compatible.
- [ ] M2 validates inert, bounded, versioned Capability Packs.
- [ ] `rust-deep` has documented deterministic and nondeterministic profiles.
- [ ] `secure-repository` binds tool, rule, and database provenance.
- [ ] M5 reaches an evidence-backed findings-contract decision.
- [ ] Documentation and onboarding match observed behavior.
- [ ] A fresh Mac user can inspect and run a supported pack through the
      documented entry point.
- [ ] Stable release evidence, rollback, tag, and publication are terminal.

Completion is unproven until every applicable item has authoritative evidence.
