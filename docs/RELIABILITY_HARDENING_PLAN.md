# Commit CI Preflight — Reliability Hardening Plan

## 1. Document control

| Field | Value |
| --- | --- |
| Status | **IN PROGRESS — T0/T1 and macOS-v4, multi-runtime, and admission-ownership slices are on main; T2 has exact-head qualification on `8eaeb94`; T3 has exact-head macOS/OrbStack qualification on `1cead1e`; acceptance and T4-T11 remain** |
| Baseline date | 2026-08-14 |
| Baseline branch | `main` |
| Baseline commit | `641a0eed29075696eec1e4e07f8de554f6ce9459` |
| Current delivery anchor | `origin/main` at `73ec2a32f730b075cd95f59e084e0ce80de609e9` (verified 2026-08-19) |
| Current release line | `v0.1.0-rc.1` prerelease |
| Owner and release authority | Marco Porcellato |
| Scope | Correctness, crash consistency, runtime ownership, evidence fidelity, recovery, and qualification |
| Out of scope | Marketing, visual design, package publication, billing, signing-key creation, and unrelated product features |

This document records the repository state after resource observation history v2
and defines the ordered work required to move Commit CI Preflight from a careful
A0 receipt producer to a locally reliable execution-and-evidence system.

It complements, but does not rewrite, the shipped beta history in
[`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) and the longer-term product
direction in [`PRODUCT_ROADMAP.md`](PRODUCT_ROADMAP.md). When this document is
adopted, it is the source of truth for reliability hardening and stable-release
qualification.

## 2. Evidence boundary

The baseline combines:

- direct source and documentation inspection at the exact commit above;
- the repository's recorded `cargo fmt`, `cargo check`, 115 library tests,
  13 binary tests, CLI smoke evidence, and native benchmark evidence;
- an independent static technical review of runtime, cache, admission,
  supervisor, receipt, verifier, workspace, resource-history, and GitHub-gate
  paths.

The independent review did **not** dynamically reproduce every proposed failure
mode against a real Docker daemon. In particular, the orphan-container risk is
architecturally credible but remains a hypothesis until a repeatable native
regression test is executed in a disposable owned environment. T0 records the
current one-shot Docker client contract deterministically; T3 owns the real
daemon lifecycle and its native failure qualification.

At the baseline commit:

- Clippy was not available for the latest resource-history tranche;
- the heavy OrbStack qualification was not run because the live macOS resource
  gate denied admission;
- no PASS is inferred for either missing gate;
- branch-protection rules and complete Dependabot state were not independently
  available to the static reviewer.

Source review, unit tests, an inner receipt, or a partial native run are not a
substitute for a terminal exact-commit qualification receipt.

## 3. Current repository snapshot

### 3.1 Implemented strengths to preserve

The current `0.1.0` Rust implementation already provides:

- strict TOML and JSON parsing with unknown-field rejection;
- deterministic normalization, canonical JSON, and SHA-256 receipt IDs;
- explicit argv commands rather than an implicit shell;
- bounded configuration, receipt, output, timeout, CPU, memory, and PID inputs;
- read-only repository mounting with declared writable cache/artifact paths;
- Unix process groups and Windows Job Objects for local child containment;
- cancellation and stable failure classes;
- a verifier that is logically separated from runtime, cache, and workspace;
- exact-commit, image, platform, freshness, and required-check policy checks;
- mutation tests that invalidate every altered receipt leaf;
- fail-closed GitHub evidence retrieval and exact-head status publication;
- host-wide single-slot admission and strict macOS resource sampling;
- privacy-bounded local resource history v2 with no admission authority;
- honest A0 trust language and explicit non-claims.

These controls remain invariants throughout the hardening programme. A later
schema or crate split must not weaken v1 parsing bounds, privacy, deterministic
ordering, exact-head binding, or unsupported-platform honesty.

### 3.2 Current architecture

The project remains one Rust package with a single production binary. Its main
modules are:

| Module | Current responsibility | Hardening pressure |
| --- | --- | --- |
| `config` | Strict config, normalized execution plan, plan digest | Environment values and full trusted-plan binding are incomplete |
| `runtime` | Docker probe and `docker run --rm --init` argv rendering | CCP owns the client process, not proven daemon-side container lifecycle |
| `process` | Spawn, timeout, cancellation, output capture, process-group/Job Object cleanup | Cleanup is procedural; pipe joins and full-stream evidence need stronger bounds |
| `workspace` | Live repository and writable mount preparation | The mounted bytes may include ignored local state outside commit identity |
| `cache` | Content-addressed entries and `.complete-v1` markers | Writable complete entries can be damaged without invalidating the old marker |
| `admission` | Persistent single-slot queue and ticket recovery | Some locks and persistent writes are not fully deadline- and crash-safe |
| `run` | End-to-end orchestration and receipt publication | No persistent typed state machine or run journal spans all phases |
| `receipt` | Receipt v1 types, canonicalization, integrity | Declared plan and artifacts are not fully evidenced |
| `verify` | Integrity and repository-policy evaluation | It trusts the declared configuration digest instead of reconstructing the plan |
| `resource` | macOS pre-start and in-run resource policy | Watchdog cadence and capability evidence need tighter semantics |
| `resource_history` | Bounded advisory JSONL history | Type-level writer serialization and corrupt-record recovery are incomplete |
| `main` | CLI parsing, orchestration, output, and exit mapping | It is becoming an architectural hub and compiles into the GitHub verifier path |

### 3.3 Current assurance classification

The current public claim remains **A0 integrity and repository-policy
assertion**. It does not establish producer identity, host trust, or truthful
execution by a hostile producer.

The independent static review scored the repository strongly for strict
configuration and receipt integrity, but materially lower for daemon-side
runtime ownership, commit-to-byte fidelity, cache transactions, crash recovery,
and plan-to-receipt binding. Those scores are review findings, not benchmark or
qualification receipts.

### 3.4 Stable-release position

The repository is suitable for controlled use by trusted operators who retain
remote fallback and understand the A0 boundary. It is **not yet qualified as the
sole CI gate**. Stable-release language must remain blocked until the exit gates
in section 11 are proven on exact commits.

### 3.5 Documentation alignment debt

At the live anchor, `IMPLEMENTATION_PLAN.md` still labels the post-beta resource
observation tranche as “In progress”, although PRs #29 and #30 are present on
`main`. The plan remains useful as historical delivery evidence, but that row
must be reconciled in a separate documentation change. `PRODUCT_ROADMAP.md`
continues to govern productization; this document changes only the reliability
critical path and stable-release gates.

### 3.6 Live reconciliation — 2026-08-19

The authoritative remote state was refreshed before this plan update:

- `origin/main` is `73ec2a32f730b075cd95f59e084e0ce80de609e9`, including the
  merged admission-ownership correction from PR #38;
- PR #34 remains open and draft at head
  `2b63e4a4a77e7e792c7af1c2ad422138d81835f9`; GitHub reports its base anchor as
  `f9db80df2efc05fbc66276e6b61e04db2db24959`, while the current remote `main`
  is newer, so the PR is `CONFLICTING` and not mergeable;
- PR #34's only recorded workflow result is `SKIPPED`, therefore it is not
  qualification evidence;
- a read-only merge simulation against current `main` found conflicts in
  `schema/receipt-v2.schema.json` and `src/run.rs`;
- the local branch `codex/reliability-hardening-t2` is clean at
  `a94e0291a7ab8f631585eee672d48223d11d247d` and contains five commits beyond
  the public PR head, but those commits are not evidence of a published PR and
  must be preserved without being overwritten;
- exact-head T2 receipt qualification is recorded at `8eaeb94` with all five
  required checks PASS, independent verification PASS, source snapshot entry
  count 146, and receipt ID
  `sha256:236307559ef7695f0d5da2f363fa713edd65184dd9b38b21447734de508f0505`;
- T2 native crash/power-loss and non-macOS qualification remain pending; the
  exact-head receipt is evidence for this candidate only and does not imply
  PR acceptance or stable-release qualification.

This reconciliation is a planning fact set, not permission to merge PR #34 or
to treat its unpublished local continuation as reviewed mainline code.

### 3.7 Urgent macOS-v4 watchdog correction

This correction is reliability work under P1-8, not predictive admission or a
new product capability. The local v2 history contained 84 records, including 14
resource-pressure outcomes. Ten of those outcomes could only have crossed the
compressor criterion because their minimum available and reclaimable memory
and maximum swap never crossed another soft threshold. The latest affected JIT
shadow retained 42% available memory, at least 8,126,005,248 reclaimable bytes,
zero swap, and reached 43.4% compressor occupancy before `macos-v3` cancelled
it. These aggregates contain no repository, command, commit, user or machine
identity.

Approved `macos-v4` invariants:

- keep the reviewed `macos-v3` pre-start available, reclaimable and swap limits;
- never deny an otherwise healthy pre-start sample or cancel an in-progress run
  for compressor occupancy alone;
- require at least 70% compressor occupancy plus another pressure signal for a
  compressor-driven immediate pre-start denial;
- require at least two converging soft signals for 15 consecutive two-second
  samples;
- treat 8 GiB swap, critically low available/reclaimable memory, or 70%
  compressor occupancy plus another signal as immediate hard pressure;
- use a bounded 30-second swap-growth signal without retaining the full time
  series;
- append the exact hard/soft trip sample to compatible local v2 history;
- keep current fail-closed receipt and outer-result semantics unchanged in this
  tranche. A future rule allowing a late soft trip after completed execution to
  preserve PASS requires a separate assurance decision and tests.

The deterministic acceptance fixture reproduces the observed 42% available,
8,126,005,248 reclaimable, 16,777,625,600 compressed and zero-swap sample and
must continue. Separate tests cover every hard signal, compound soft duration,
healthy reset, swap trend, first-trip preservation, strict history validation,
and privacy exclusions. Native OrbStack qualification remains required after a
host restart and fresh `Admit` decision.

Post-restart qualification also confirmed that two pre-existing CLI contracts
acquire real macOS admission. Running either alone under `Admit` passed, while
running it after the deterministic suite could receive exit code 6 solely from
the changed live compressor sample. To preserve the T0 invariant that the
default suite is independent of host pressure, those two tests are explicit
native opt-in gates with documented exact commands. An ignored default-suite
result is not native PASS evidence. That same sample motivated the approved
compound pre-start correction: it retained 40% available memory,
7,594,246,144 reclaimable bytes and zero swap, so compression alone was not a
credible reason to discard the next run before the watchdog could observe it.

### 3.8 T2 publication checkpoint — 2026-08-19

The T2 implementation and its documentation reconciliation are now published
as draft PR #40, stacked on the clean draft PR #39. The current T2 head is
`678be576e261b4ee5a62fd105387489ebc5937d5`; its conflict-resolution merge
preserves both the live-plan reconciliation and the immutable-snapshot source
work.

The preceding exact source commit `c6ad72bb067651cee6fcbf540fb3a953b1da432a`
has a valid v2 receipt and independent verification, but that receipt is not
evidence for the current head. A fresh exact-head run was not started because
the local CCP control plane returned `resource decision=unknown` and failed to
read `queue.lock`; the fail-closed policy therefore prevented heavy execution.
The current head remains **PUBLISHED / NOT QUALIFIED** until a fresh receipt,
independent verification, and post-run cleanup evidence exist.

T3 must not start as an accepted dependency milestone until T2 has that exact
head evidence and the stacked publication path has been reviewed. No skipped
GitHub check or historical receipt changes this boundary.

## 4. Reliability gap register

### 4.1 P0 — must be closed before new product capability

| ID | Gap | Current failure mode | Required invariant |
| --- | --- | --- | --- |
| P0-1 | Daemon-side runtime ownership | Killing or losing the local Docker CLI does not prove that its container stopped or was removed | A run cannot release admission or seal PASS until every CCP-owned container is absent by daemon inspection |
| P0-2 | Commit-to-byte fidelity | A clean Git checkout can still expose ignored files, stale generated state, local symlinks, LFS differences, or submodule drift through the live source mount | Every attestable run mounts a CCP-owned immutable snapshot and records its canonical source digest |
| P0-3 | Transactional cache correctness | A previously complete writable cache can be partially modified by a failed run while retaining its old complete marker | A failed run cannot alter the last-known-good complete generation |
| P0-4 | Trusted plan binding | The verifier compares a declared configuration digest but does not prove that receipt argv, limits, mounts, environment, and dependencies represent that plan | The trusted verifier reconstructs the plan and compares every normative execution field |
| P0-5 | Supervisor cleanup completeness | Some monitor, pipe-reader, join, or escaped-session failures can bypass or outlive procedural cleanup | Every exit path has bounded containment finalization and typed cleanup evidence |
| P0-6 | Admission crash consistency | Blocking locks, partial tickets, or a truncated counter can exceed the public deadline or poison recovery | Every admission operation is end-to-end bounded and every partial state is recoverable or quarantined |

No new telemetry intelligence, predictive admission, signing, history format,
or `guard exec` capability should enter `main` until regression tests for all six
P0 gaps exist and the affected tranche's invariant is closed.

### 4.2 P1 — required before strong stable claims

| ID | Gap | Required outcome |
| --- | --- | --- |
| P1-1 | Environment values are not bound | Fixed values enter the plan digest; secrets remain remote-only; runtime-internal paths are assigned deterministically |
| P1-2 | Docker mount grammar edge cases | Structured rendering or validation rejects every ambiguous delimiter/control sequence |
| P1-3 | Artifact declarations are not evidence | Required artifacts are regular, bounded, producer-bound, and represented by canonical size/digest manifests |
| P1-4 | Disk budget is advisory only | Preflight reserves receipt/journal space and enforces owned cache/artifact ceilings without deleting unowned data |
| P1-5 | Docker memory semantics vary | RAM/swap policy, daemon capability, context, image ID, and pull behavior are explicit and verified |
| P1-6 | Receipt/cache ordering is not transactional | Cleanup and source revalidation precede evidence sealing; cache promotion cannot invalidate a valid receipt |
| P1-7 | Atomic writes are not fully crash-durable | Shared cross-platform durable filesystem primitives sync files and parent directories and close symlink/TOCTOU gaps where supported |
| P1-8 | Watchdog cadence is weaker than its label | Sample period, probe deadline, maximum sample gap, failures, and snapshot age are measured and bounded |
| P1-9 | Resource history relies on external serialization | The store owns its writer lock, validates records fully, and quarantines corruption without blocking future append |
| P1-10 | Failure taxonomy is too coarse | Test failure, timeout, user cancellation, resource cancellation, runtime loss, cleanup uncertainty, and infrastructure failure remain distinct |
| P1-11 | Producer compatibility is weakly constrained | Policy can reject incompatible or revoked producer versions/builds/contracts |
| P1-12 | Deterministic tests depend on live host state | Unit and contract suites use fake probes, clocks, runtimes, admission, and faulting filesystems; live qualification is separate |
| P1-13 | Verifier separation is logical, not physical | A minimal verifier crate/binary has no runner, Docker, cache, migration, benchmark, or resource dependencies |
| P1-14 | Evidence branches can change after PASS | Evidence identity is immutable or every evidence-ref change triggers exact revalidation/revocation |

## 5. Target architecture

### 5.1 Dependency direction

The target workspace is:

```text
crates/
  ccp-core/             normalized plans, receipts, policy, schemas, digests
  ccp-verifier/         bounded trusted verification only
  ccp-git-snapshot/     immutable commit materialization and source manifest
  ccp-supervisor/       process containment, output drain, deadlines, cleanup
  ccp-runtime-docker/   daemon-owned container lifecycle adapter
  ccp-state/            durable filesystem, journals, admission, cache recovery
  ccp-runner/           typed execution state machine
  ccp-cli/              Clap, human/JSON rendering, stable exit codes
```

Dependency rules:

```text
ccp-core <- ccp-verifier
ccp-core <- ccp-git-snapshot
ccp-core <- ccp-supervisor
ccp-core <- ccp-runtime-docker
ccp-core <- ccp-state
ccp-core + adapters <- ccp-runner <- ccp-cli
```

`ccp-verifier` must not depend on the runner or any runtime/state adapter. The
split is incremental: no broad file move is accepted before contract tests pin
the old behavior.

### 5.2 Typed run state machine

The runner should make invalid transitions unrepresentable:

```text
Planned
  -> SnapshotBound
  -> Admitted
  -> RuntimeOwned
  -> Executed
  -> RuntimeCleanupVerified
  -> SourceRevalidated
  -> EvidenceSealed
  -> EvidencePersisted
  -> CachePromoted
```

Only `RuntimeCleanupVerified` may advance toward receipt sealing. Cleanup
uncertainty, lost runtime ownership, stale source, or journal persistence failure
must produce a non-PASS terminal state.

### 5.3 Persistent run journal

Every attestable run records a privacy-minimized journal containing:

- run ID and contract version;
- current phase;
- source commit, snapshot identity, and snapshot path under CCP ownership;
- daemon-side runtime IDs and exact ownership labels;
- cache staging generations;
- receipt temporary path;
- required recovery action;
- no secret, command output, repository contents, or environment values.

Journal transitions use temp file, file sync, atomic replace, and parent-directory
sync. Startup scans unfinished journals and classifies them as automatically
recoverable, quarantined, or operator-required. Recovery operates only on exact
CCP ownership markers and labels.

## 6. Delivery strategy

### 6.1 General rules

Each tranche must be small, reversible, and independently reviewable. Every PR:

1. starts from current `origin/main` in an isolated branch/worktree;
2. adds regression evidence before changing the affected invariant;
3. preserves v1 parsing and verification unless a documented compatibility
   transition explicitly changes it;
4. distinguishes deterministic tests from live qualification;
5. updates architecture, threat model, relevant contract, and changelog;
6. records exact commands, commit, platform, terminal status, and residual risk;
7. does not claim PASS from a skipped, denied, partial, or inner-only result.

### 6.2 Receipt compatibility

Receipt v1 remains readable during the transition. Receipt v2 is introduced
with explicit schema and producer compatibility rules:

- v1 verification remains available for historical evidence;
- v2 is required for new strong execution-fidelity claims;
- policy selects accepted schema and producer ranges explicitly;
- no v1 receipt is silently upgraded or reinterpreted as v2;
- golden vectors and mutation coverage exist for both versions during migration;
- stable release requires v2 evidence for all required hardening gates.

## 7. Ordered roadmap

### PR T0 — Characterization and deterministic fault seams

Deliverables:

- deterministic characterization tripwires for all six P0 scenarios;
- an inventory of the existing runtime, resource, admission, clock, process,
  cache, and workspace seams;
- deterministic unit/contract suite separated from live host qualification;
- explicit test classification: unit, contract, integration, native, chaos;
- baseline compatibility fixtures for receipt/config/policy v1.

T0 must not add an unused generic filesystem abstraction. T1 introduces the
shared `FaultInjectingFileSystem` together with the first production consumer,
and later tranches reuse that one seam. The current test-only fake supervisors,
resource runners, clocks, coordinator roots, and filesystem fixtures remain
local to the components they characterize.

Mandatory scenarios:

1. ignored source input changes behavior at the same commit;
2. a failed run mutates a previously complete cache;
3. a receipt with altered argv can still carry the expected declared digest;
4. a partial ticket/counter state blocks or confuses admission;
5. a monitoring or pipe failure reaches cleanup on every path;
6. Docker client death leaves a daemon-owned container discoverable.

Source-side checkpoint on `codex/reliability-hardening-t0`:

- all six gaps have named `characterizes_*_before_tN` tripwires;
- the testing and fault-injection contract is linked from the public docs;
- `cargo test --all-targets --all-features`: 197 passed, 0 failed, 0 ignored;
- `cargo +stable clippy --all-targets --all-features -- -D warnings`: PASS;
- `cargo fmt --all -- --check` and `git diff --check`: PASS;
- Docker/OrbStack native reproduction: **NOT RUN** because host resource
  admission was `unknown`; no native PASS is claimed.

Exit gate:

- tests reproduce the gaps without weakening production assertions;
- ordinary `cargo test --all-targets --all-features` does not depend on live
  macOS memory pressure, OrbStack, or Docker availability;
- live tests are opt-in, clearly named, and never reported as run when skipped.

### PR T1 — Durable state substrate and run journal

Deliverables:

- `durable_fs` primitives for create-new, atomic replace, parent sync,
  quarantine, and owned-tree removal;
- fail-at-N filesystem harness;
- versioned run journal and phase transitions;
- startup `recover status` classification with no mutation;
- bounded `recover apply <run-id>` restricted to exact owned state;
- typed recovery outcomes and privacy tests.

Exit gate:

- failure at every filesystem operation never produces partial PASS evidence;
- unfinished states are recoverable, quarantined, or explicitly operator-bound;
- ENOSPC leaves a readable last-known state and an actionable bounded error;
- Unix and Windows replacement semantics have platform-specific tests.

Source-side checkpoint on `codex/reliability-hardening-t1`:

- immutable create-new journal events are active on the real `run` lifecycle;
- receipt v1 remains unchanged and separate from recovery state; T2 adds a
  strict v2 publication path for snapshot-backed runs without rewriting v1;
- `recover status` opens only an initialized managed cache and performs no
  mutation;
- `recover apply <run-id>` quarantines one exact CCP-owned unfinished journal;
- root/run-bound opaque ownership tokens prevent static marker reuse, and a
  retry recognizes an already completed quarantine after a directory-sync
  failure;
- malformed, foreign, ambiguous, terminal, or corrupt state fails closed;
- deterministic Unix tests cover operation-level storage failures and
  old-or-new atomic replacement; Windows replacement of an existing file is
  explicitly unsupported rather than implemented as remove-then-rename;
- Windows-native crash and power-loss qualification is **NOT RUN** and remains
  required before a cross-platform qualification claim.

### PR T2 — Immutable source snapshot and byte identity

Status (reconciled and exact-head qualified 2026-08-19): deterministic source implementation exists in
the current T2 candidate and in the earlier PR #34 continuation, but the public
PR #34 is stale and conflicting against current `main`. The reconciled T2
candidate is published as draft PR #40, stacked on the documentation
reconciliation PR #39, and is not yet an accepted mainline milestone. The
candidate preserves the independent local continuation while resolving the
receipt-v2 schema and run-orchestration conflicts against the current delivery
anchor. The exact head `8eaeb94` is **QUALIFIED FOR THIS CANDIDATE** by the
receipt and independent verifier recorded above. PR acceptance, native
crash/power-loss, and non-macOS qualification remain pending and are not
represented as PASS.

Deliverables:

- ADR selecting Git-object materialization, `git archive`, or a protected
  detached worktree strategy;
- CCP-owned exact-commit snapshot outside the user's working tree;
- canonical manifest over path, mode, blob OID, and submodule OID;
- `source_snapshot_digest` in the execution contract and receipt v2;
- explicit submodule, Git LFS, executable-bit, symlink, and sparse-checkout policy;
- read-only runtime mount of the snapshot only;
- bounded snapshot cleanup/recovery through the journal.

Exit gate:

- ignored files, `.env`, generated files, local cache directories, and IDE state
  cannot affect attestable execution;
- the same supported Git tree produces the same source digest;
- unsupported LFS/submodule states fail closed before admission;
- source identity is revalidated after execution and before receipt sealing.

### PR T3 — Daemon-owned Docker lifecycle

Status (exact-head qualified candidate 2026-08-19): the isolated T3 branch
implements the shell-free Docker lifecycle adapter and deterministic
command/ownership fixtures. The adapter refuses broad cleanup, uses a fresh
cleanup token after cancellation, and returns a typed non-PASS result unless
final not-found inspection succeeds. Exact-head macOS/OrbStack qualification
for `1cead1e` passed all five required checks, independently verified the v2
receipt, and released admission with no remaining container. Receipt-v2 runtime
evidence, total lifecycle deadlines, native failure-path expansion, and
admission/journal release coupling remain explicit follow-up gates before T3
acceptance.

Local verification checkpoint (2026-08-19): `cargo fmt --all -- --check`,
`cargo check --locked --all-targets --all-features`, the focused runtime suite
(11 tests), and the full serial locked suite (157 library tests plus all
repository test targets) passed. One pre-existing native benchmark test and one
guard-exec test remain ignored by their repository admission contract. Local
Clippy was unavailable because the installed Rust toolchain lacks the
`cargo-clippy` component; the pinned OrbStack exact-head run nevertheless
executed Clippy successfully. The exact T3 receipt ID is
`sha256:5603808e88b740ba57aa1878679ce2a3a0824b0ceea67a8495a8b592bd421249`.

Deliverables:

- ADR for structured Docker CLI lifecycle versus Docker Engine API;
- deterministic container name and labels derived from run/check identity;
- create, start/attach, inspect, graceful stop, hard kill, forced remove, and
  final not-found verification;
- daemon/context/image identity recorded in privacy-safe runtime evidence;
- `recover runtime` limited to exact CCP labels and journal ownership;
- admission release and receipt sealing blocked until container absence is proven.

Exit gate:

- killing the Docker client does not orphan an untracked CCP workload;
- daemon restart, attach loss, stop timeout, kill failure, inspect failure, and
  remove failure all produce typed non-PASS outcomes;
- a real Docker/OrbStack integration test proves container absence after every
  supported terminal path;
- no broad label search or removal can affect non-CCP containers.

### PR T4 — Supervisor and output evidence hardening

Status (candidate slice 2026-08-19): process readers now retain only a bounded
preview while hashing and counting the complete stdout/stderr streams. Receipt
output digests bind those full-stream digests and byte counts, so equal previews
with different suffixes cannot collide. A single wall deadline now bounds the
execution and cleanup sub-budgets; pipe-reader joins are bounded; and monitor or
force-stop failures still attempt descendant cleanup before failing closed.
Deterministic process tests cover these invariants and the full locked Rust
suite passes. This does **not** yet close T4: RAII finalization, stronger
interruptible drain semantics, native fault-path qualification, and exact-head
OrbStack evidence for this new candidate remain pending.

The implementation commit `d7d72e8` was then qualified independently on genuine
macOS arm64 with OrbStack 29.4.0: the exact-head receipt run ID was
`sha256:712e392ab0d1456803b9b1acff249dc8d860a8891655f637639bd33308941ff4`,
all five required checks were `PASS`, the source snapshot contained 147 Git
objects with manifest
`sha256:bb7501143f7834e736e92a594912fd9c700ecf54c40e019fcdacc6a368b57226`,
and independent integrity/policy verification passed. Admission was released
with no remaining Docker container. This evidence qualifies that exact source
commit only; any later candidate commit, including documentation-only changes,
requires its own exact-head receipt before acceptance.

Deliverables:

- RAII containment/finalization object;
- one total wall deadline with explicit execution and cleanup sub-budgets;
- bounded pipe-reader joins and interruptible drain;
- full-stream stdout/stderr digest and byte count;
- bounded preview and nonblocking display channel separated from pipe drain;
- typed termination reason and cleanup state;
- tests for escaped sessions, blocked writers/readers, infinite output,
  `try_wait` errors, panic boundaries, and ignored graceful termination.

Exit gate:

- every return path attempts bounded cleanup exactly once;
- terminal speed cannot block pipe draining;
- two outputs with the same retained prefix but different suffixes have different
  full-stream digests;
- the public timeout bounds execution, cleanup, drain, removal, and verification.

### PR T5 — Generational transactional cache

Status (candidate slice): the runtime cache path now prepares an isolated,
manifest-bound `.staging-*` generation per declared cache, copies the previous
complete data with symlink rejection, and promotes only a validated generation
after the run's checks pass. The owning prepared handle removes an unfinished
staging generation on ordinary failure, while a failed generation cannot alter
the previous complete data. This is not yet the T5 exit gate: cross-process
crash recovery, journaled multi-cache promotion/rollback, and exact-head
qualification remain pending.

The next recovery slice adds owned advisory OS locks for each prepared cache
entry and for the promotion boundary. A second process cannot prepare the same
entry or enter promotion concurrently, and a process exit releases the lock
without requiring unsafe stale-lock deletion. The locks do not by themselves
recover an interrupted multi-cache promotion; that journal and recovery path
remains pending.

The journal candidate now records every prepared cache entry, its prior marker
and manifest bytes, and an owned backup name before promotion begins. All
entries are promoted while the journal remains present; backups and staging
directories are removed only after the complete set succeeds. A later promoter
can recover a prepared or partially promoted journal only after acquiring the
same entry locks; ambiguous state returns a hard error and leaves evidence in
place. Durable manifest replacement now uses the existing durable filesystem
primitive on Unix/macOS, with the conservative legacy fallback retained on
non-Unix targets. On macOS, a complete generation attempts an APFS
`clonefile` copy after bounded tree validation, then falls back to the same
deterministic symlink-rejecting recursive copy when the filesystem does not
support cloning.

The public cache API now returns `CachePromotionOutcome` separately from check
evidence. Empty cache plans report `not_attempted`, successful promotion
reports `promoted`, and promotion uncertainty remains an error; no cache
failure is converted into a passing check result.

This remains a candidate slice: exact-head native qualification, crash-point
subprocess evidence, and platform-specific clone behavior remain pending. A
clone failure is never treated as a cache success; it either takes the bounded
copy fallback or returns a hard error, and an incomplete preparation cannot
replace the previous complete generation.

Deliverables:

- immutable last-known-good generations;
- per-run staging generation cloned by reflink/clonefile when supported, with
  safe copy/rebuild fallback;
- versioned manifest containing plan, image, toolchain, generation, and state;
- per-entry locking, quarantine, and stale staging recovery;
- journaled multi-cache promotion;
- explicit cache promotion outcome separate from check evidence.

Exit gate:

- a failed, cancelled, timed-out, or crashed run cannot change `current`;
- completion marker is written last after durable content/manifest persistence;
- partial multi-cache promotion is recoverable or rolled back deterministically;
- no cleanup operation removes data not provably owned by CCP.

### PR T6 — Crash-safe bounded admission

Status (candidate slice): admission now uses one cooperative deadline-aware
lock primitive for queue acquisition, status snapshots, ticket publication,
slot contention, and release cleanup. Ticket records are fully written and
synced in a staging name before publication; the legacy high-water counter is
updated with durable replacement rather than truncation. A valid unlocked
ticket with no lease, an unlocked malformed ticket, or an abandoned staging
record is quarantined reversibly under the CCP-owned coordinator root. Locked,
foreign, malformed-with-live-lock, and otherwise ambiguous states still fail
closed and remain in place. The heartbeat stop path is interruptible, so
release does not inherit the full heartbeat interval.

The read-only status command has an explicit bounded timeout and continues to
report the cross-activity visibility note. A local process list remains
insufficient evidence of global inactivity. This slice does not yet claim the
T6 exit gate: all filesystem calls are not interruptible at the OS level,
Windows-native atomic replacement semantics and crash-point subprocess
receipts remain pending, and the full admission state-machine qualification is
still required.

Deliverables:

- one deadline-aware, cancellation-aware lock primitive used by every queue,
  metadata, ticket, slot, status, and release operation;
- atomically committed ticket records;
- robust counter replacement or counter elimination;
- recovery/quarantine matrix for valid/malformed and locked/unlocked tickets;
- model/state-machine tests plus subprocess crash points;
- end-to-end timeout accounting.

Exit gate:

- public admission timeout bounds metadata setup, allocation, queue wait, slot
  acquisition, stale recovery, and release;
- a crash at every ticket/counter phase cannot deadlock future status/acquire;
- at most one slot owner exists;
- cancellation removes only the caller's own committed ticket.

### PR T7 — Trusted plan binding and receipt v2

Status (local implementation tranche): policy `1.1` reconstructs a normalized
plan only from a regular non-symlink configuration selected relative to the
trusted policy. It accepts receipt v2 only, independently compares bounded
field pointers without values, constrains producer name/version and snapshot
strategy, and keeps v1 policy/receipt behavior historical and separate. The
root policy has migrated locally to `1.1`. This is deterministic source evidence
only until the exact candidate has the required independent receipt and review;
it does not claim signing, producer identity, or native qualification.

Deliverables:

- verifier loads trusted configuration and policy from the trusted base;
- normalized trusted plan is reconstructed independently;
- receipt v2 contains the canonical plan or complete per-check plan digests;
- field-by-field comparison for argv, working directory, dependency DAG,
  required status, timeout, network, fixed environment, image, mounts, artifacts,
  limits, source snapshot, and producer compatibility;
- explicit supported/revoked producer contract;
- v1/v2 schemas, golden vectors, migration, and downgrade tests.

Exit gate:

- changing any normative execution field while retaining the expected declared
  digest is rejected;
- verifier has no dependency on runtime, cache, workspace, resource guard, or
  process supervision;
- signing remains deferred and cannot mask a plan mismatch.

### PR T8 — Environment, artifact, disk, and runtime-resource contracts

Status (local artifact, capacity, and runtime-capability sub-slices): typed contracts now bind artifact path,
kind, byte and entry limits, and producer check into the normalized plan.
Snapshot-backed v2 runs observe only the CCP-owned writable artifact mount
after source revalidation and before barrier, cache promotion, and receipt
sealing. They record a canonical final-state digest and fail closed on missing,
symlinked, escaped, replaced, oversized, or over-count output. Initial-state
evidence, inode reservation, owned cache-total enforcement, and native
qualification remain pending. Schema 1.2
now declares a bounded disk-capacity allowance and preflights free bytes on the
CCP-owned cache-root filesystem before Git, runtime, or workspace work; it
never deletes data and records no host capacity sample in receipts. Opt-in
schema 1.3 binds no-pull and disabled-swap runtime policy, requires bounded
read-only daemon/context/local-image preflight before journal, snapshot,
workspace, or container mutation, and seals privacy-bounded matching evidence
in receipt v2. Delimiter/control/path regression tests reject ambiguous mount
grammar without shell escaping. These are deterministic source-level facts,
not native runtime qualification. This status does not close T8 or its exit
gate.

**T8-C closeout boundary:** schema policy, bounded preflight, receipt binding,
and mount grammar are source-verified with deterministic fake ports. They do
not replace exact-commit native receipts and independent verification for
macOS/OrbStack, Linux/Docker, or Windows. Initial artifact-state evidence,
inode reserve, and owned cache-total enforcement remain open T8 work. A
Docker, OrbStack, CCP, container, network, or release claim must not be inferred
from the source suite.

Deliverables:

- environment classes: fixed, runtime-internal, and remote-secret-only;
- no implicit inherited environment in attestable checks;
- required artifact manifest with type, size, digest, producer check, initial
  state, final state, and directory-entry bounds;
- disk/inode preflight, receipt/journal reserve, and owned growth ceilings;
- explicit Docker RAM/swap mode, `--pull=never` attest phase, daemon capability,
  context identity, and resolved image ID/digest;
- structured mount rendering or complete delimiter/property tests.

Exit gate:

- relevant host environment variations are fixed, rejected, or recorded;
- missing/escaped/symlinked/oversized required artifacts are non-PASS;
- disk exhaustion cannot create a false-complete cache or partial PASS receipt;
- requested memory/swap limits are verified as supported, not merely rendered.

### PR T9 — Physical verifier split and GitHub evidence immutability

Deliverables:

- incremental workspace split beginning with `ccp-core` and `ccp-verifier`;
- minimal verifier binary with a reviewed dependency allowlist;
- GitHub gate builds or retrieves only the trusted verifier artifact;
- evidence status records source commit, evidence commit/blob digest, receipt ID,
  verifier digest, policy digest, and evaluation time;
- evidence-ref push/rewrite/delete triggers revalidation or revocation;
- ruleset requirements and unverifiable administrative assumptions documented.

Exit gate:

- gate dependency graph contains no runner/runtime/cache/resource/migration code;
- changing evidence after PASS cannot leave an unqualified success status silently;
- fork and untrusted-contributor behavior remains fail-closed;
- no signing or registry publication occurs without separate authorization.

### PR T10 — Resource guard and history correctness

Deliverables:

- absolute-deadline sampling with interruptible wait;
- sample count, maximum gap, probe failures, age, and watchdog health;
- dedicated resource-history writer lock and complete semantic validation;
- corrupt-history quarantine and bounded append recovery;
- history persistence outside the heavy slot or under a strict terminal deadline;
- documentation that reports measured cadence rather than a nominal sleep interval.

Exit gate:

- rapid pressure and stale/failed probes have deterministic policy outcomes;
- advisory history cannot block or alter admission;
- concurrent writers cannot lose records;
- one corrupt record cannot permanently disable future history.

### PR T11 — Chaos, native qualification, and stable-release gate

Deliverables:

- filesystem fail-at-N suite, daemon restart, client kill, disk-full, cleanup
  timeout, blocked pipe, power-loss simulation, and recovery replay;
- property tests for paths, mount rendering, normalization, DAGs, timestamps,
  size parsers, image references, policy/receipt compatibility, and journals;
- fuzz targets for config, policy, receipt, GitHub Actions YAML, resource output,
  cache manifests, admission tickets, and journals;
- native full-run evidence for macOS arm64 + OrbStack, Linux x86_64 + Docker
  Engine, and Windows x86_64 process/runtime semantics;
- exact-commit release checklist and updated threat model/non-claims.

Exit gate:

- all section 11 criteria are terminally proven;
- unresolved P0/P1 findings are zero or explicitly downgrade the release claim;
- stable publication, package registries, signing, or distribution remain separate
  owner authorization gates.

## 8. Gap-to-tranche traceability

| Gap | Primary tranche | Supporting tranches |
| --- | --- | --- |
| P0-1 daemon lifecycle | T3 | T0, T1, T4, T11 |
| P0-2 source bytes | T2 | T0, T7, T11 |
| P0-3 cache transaction | T5 | T0, T1, T8, T11 |
| P0-4 plan binding | T7 | T0, T2, T8, T9 |
| P0-5 supervisor cleanup | T4 | T0, T1, T3, T11 |
| P0-6 admission crash safety | T6 | T0, T1, T11 |
| P1 environment/artifacts/disk/memory | T8 | T2, T5, T7, T11 |
| P1 durability/journal | T1 | T3, T5, T6, T11 |
| P1 watchdog/history | T10 | T1, T11 |
| P1 verifier isolation/evidence mutation | T9 | T7, T11 |
| P1 deterministic/full qualification | T0, T11 | All tranches |

## 9. Test and qualification policy

### 9.1 Deterministic default suite

The default suite must run without Docker, OrbStack, live host pressure, network,
secrets, or platform-specific hardware. It includes:

- formatting and Clippy with warnings denied;
- all unit and contract tests;
- schema and golden-vector drift;
- mutation tests;
- deterministic fake-runtime/state-machine tests;
- bounded property and fuzz smoke tests;
- repository hygiene and dependency policy.

### 9.2 Live suites

Live suites are separate commands and receipts:

| Suite | Required host | Purpose |
| --- | --- | --- |
| `qualify local-macos` | macOS arm64 + OrbStack | Resource guard, daemon lifecycle, source snapshot, cache, cleanup, recovery |
| `qualify docker-linux` | Linux x86_64 + Docker Engine | Native runtime, filesystem, admission, cache, verifier/evidence path |
| `qualify windows` | Windows x86_64 | Job Object, durable filesystem, admission, CLI and supported runtime semantics |
| `qualify chaos` | Disposable owned environment | Crash, daemon restart, ENOSPC, power-loss, fault injection |

`SKIPPED`, resource denial, unavailable daemon, missing hardware, or an inner
PASS enclosed by a failed outer guard are never qualification PASS.

### 9.3 GitHub minimum independent core

CCP's own repository should retain a small remote core that adds independent
trust without rerunning adopters' heavy projects:

- formatting check;
- Clippy;
- deterministic unit/contract tests;
- schema/golden-vector drift;
- bounded fuzz/property smoke;
- minimal verifier build and self-verification.

Native and chaos runs may remain scheduled, labeled, or release-gated, but a
host admission denial must not make the deterministic core impossible to run.

## 10. Risk and decision register

| Decision | Default direction | Owner/ADR gate |
| --- | --- | --- |
| Snapshot implementation | Prefer direct Git-object/tree materialization; accept `git archive` only with explicit LFS/submodule policy | ADR in T2 |
| Docker control plane | Prefer a structured lifecycle adapter; Engine API is acceptable only after dependency/security review | ADR in T3 |
| Cache cloning | Reflink/clonefile optimization with deterministic copy/rebuild fallback | T5 design review |
| Receipt migration | Preserve v1 reading; require v2 for strong new claims | T7 contract review |
| Producer identity/signing | Deferred until execution truth and plan binding are complete | Separate owner authorization after T9 |
| Evidence storage | Keep current branch transport only with automatic revalidation; evaluate immutable OCI/custom refs later | T9 ADR |
| Stable release | Blocked until T11 and section 11 | Owner release decision |

## 11. Objective completion criteria

The reliability programme is complete only when all of the following are true:

1. No CCP-owned container remains after timeout, cancellation, client loss, or
   supported crash/recovery scenarios.
2. Every attestable run mounts only a CCP-owned immutable representation of the
   exact commit and records its source digest.
3. A failed run cannot corrupt or replace a cache generation still marked as
   complete.
4. The verifier reconstructs the trusted plan and compares every normative
   execution field.
5. Public timeouts bound total wall time, including cleanup, pipe drain,
   container removal, state persistence, and verification.
6. Crashes during persistent writes always lead to bounded recovery,
   quarantine, or explicit operator action—never indefinite blocking.
7. ENOSPC cannot produce a partial PASS receipt or false-complete cache.
8. Relevant environment values are deterministic, explicitly recorded, or
   classified as not locally attestable.
9. Required artifacts are bounded, producer-linked, and digest-verified.
10. The default full unit/contract suite is independent of live machine pressure.
11. Exact-commit full-run evidence exists for native Linux x86_64 and macOS
    arm64, plus Windows-native process/durability semantics.
12. Runtime, cache, admission, supervisor, snapshot, verifier, and journal
    changes each trigger their mandatory focused suites.
13. The GitHub gate uses a physically minimal trusted verifier.
14. Evidence mutation after PASS causes revalidation or revocation.
15. No unresolved P0 or P1 finding is hidden by release wording.

## 12. Stop conditions

Stop a tranche and report evidence when:

- its regression cannot be reproduced deterministically and the proposed fix
  would rely only on speculation;
- a runtime change requires privileged containers, Docker socket mounts inside
  project checks, or broad deletion rights;
- recovery cannot identify CCP ownership exactly;
- a schema migration would silently reinterpret historical evidence;
- a new dependency has unacceptable licensing, maintenance, or attack-surface risk;
- a native platform is unavailable but would be represented as PASS;
- branch protection, privacy, or fail-closed semantics would need weakening;
- signing, secrets, billing, package publication, or external infrastructure is
  required without fresh owner authorization.

## 13. Immediate next actions

1. Restore or wait for a healthy CCP control plane, then recheck resource and
   admission status without quarantining locks whose ownership is unknown.
2. Run the official T2 gate on exact head `678be576e261b4ee5a62fd105387489ebc5937d5`
   with the reviewed cache environment and pinned image.
3. Independently verify that fresh receipt and record post-run admission,
   runtime-cleanup, and lock-state evidence; keep the earlier `c6ad72b` receipt
   historical only.
4. Review and accept PR #39 before treating the stacked PR #40 as a current
   mainline candidate; do not merge either PR on skipped or missing evidence.
5. Start T3 only after the exact-head T2 exit gate is proven and the base/head
   relationship is refreshed. Keep telemetry intelligence, predictive
   admission, signing, and unrelated runner features frozen until then.
