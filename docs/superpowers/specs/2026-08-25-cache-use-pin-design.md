# Cache-use pin and spawn-boundary mount revalidation design

Status: proposed for owner review  
Date: 2026-08-25  
Baseline: `3fccc197e5055a2759ee7afe51b91133938ec904`  
Scope: protect CCP-managed bind sources from in-process premature cleanup and
fail closed when a source changes before Docker container creation

## Decision

CCP will reuse its existing per-entry advisory lock as the cache-use pin. It
will not add a second TTL, heartbeat, daemon, or persistent lease format.

Three linked changes close the demonstrated gaps:

1. cloned prepared generations share one final-drop cleanup owner;
2. standard `run` revalidates structured mount sources immediately before each
   `docker create` request;
3. `guard exec` gains an opt-in declaration for completed CCP cache entries
   that must remain pinned while its child is active.

The last contract is cooperative. `guard exec` remains a generic shell-free
argv supervisor and does not parse, rewrite, or attest arbitrary child argv.
An undeclared mount is therefore outside cache-pin coverage, not silently
treated as protected.

## Problem and evidence

Docker correctly rejects `--mount type=bind` when the host source is absent.
Replacing it with `-v` or source-creating behavior would hide the lifecycle
failure by materializing an empty directory and is forbidden.

The standard `run` path already prepares each cache generation under an
exclusive per-entry advisory lock. `PreparedWorkspace` owns the prepared
entries through check execution and promotion. This is sufficient ownership
for CCP-aware cache operations, provided every mutating operation honors the
same lock.

Two gaps remain in current source:

- `PreparedCacheEntry` derives `Clone`, but every clone runs staging cleanup in
  its own `Drop`. One clone can delete a staging directory while another clone
  still holds the lock and data path.
- Docker bind sources are checked when the dry-run argv is rendered. The
  rendered argv is later converted to `docker create` and executed without a
  new structured-path validation at the spawn boundary.

`guard exec` owns the host-wide heavy-work slot but its argv is opaque. That
slot prevents overlapping cooperative heavy workloads; it does not establish
ownership of a pre-resolved cache path embedded in a Docker command or nested
launcher.

The exact actor that removed the previously observed bind source is unknown.
Current CCP cache cleanup is preview-only, so the evidence does not support
attributing the deletion to CCP cleanup.

## Alternatives considered

### A. Add a second durable cache lease with TTL and heartbeat

Rejected for this tranche. The entry lock already expresses live ownership and
is released by process death. A second authority would introduce lock/lease
contradictions and recovery policy without improving Docker's path-based mount
semantics.

### B. Parse every `guard exec` argv and infer bind sources

Rejected. The child may be a script, Make target, nested runner, or non-Docker
program. Partial Docker parsing would create a false security claim and could
silently miss nested mounts.

### C. Reuse entry locks and require explicit guard declarations

Selected. It is compatible with the current cache model, works for direct and
nested launchers, adds no durable state, and keeps the boundary honest:
declared completed entries are pinned; other paths are not claimed.

## Component design

### Shared prepared-generation lifetime

`PreparedCacheEntry` remains clonable for source compatibility, but its
entry-lock handle and staging-cleanup identity move into one shared `Arc`-owned
generation state. Only the final shared owner removes a still-valid staging
generation.

The final-drop cleanup retains the current manifest checks: schema version,
key digest, plan digest, generation, and `staging` state must all match.
Promotion behavior remains unchanged. A promoted generation no longer has its
staging manifest, so final drop performs no destructive cleanup.

The shared generation state owns the entry lock. Its final `Drop` performs the
validated staging cleanup while that lock is still held; the file handle is
released only after cleanup returns.

### Structured mount-source expectations

Each `MountBinding` records an internal expected source kind:

- repository: plain directory;
- cache: plain directory;
- artifact: plain regular file or plain directory according to its declared
  artifact contract.

The artifact kind comes from `NormalizedCheck::artifact_contracts`; no second
configuration source is introduced. The source-kind field and every new
containment or generation expectation use `#[serde(skip)]` and never enter the
dry-run JSON or normalized plan digest.

`WorkspacePlanV1` also retains internal canonical containment anchors for the
repository, managed cache root, and run root. These fields are skipped from
serialized dry-run output so the existing JSON schema and privacy surface do
not change.

All standard and snapshot-backed workspace constructors populate these fields.
For a live cache mount, the expectation additionally records the exact cache
key digest, plan digest, generation, and required `staging` manifest state from
the corresponding `PreparedCacheEntry`.

The live `PreparedWorkspace` has already created all writable sources. Its plan
therefore contains complete expectations. Pure non-executing dry-run planning
may still describe absent managed sources and is not relabeled as a live
revalidation.

### Spawn-boundary revalidation

A pure workspace validator rechecks every live binding immediately before
`DockerLifecyclePlan::build` and before any `SupervisorPort` call:

1. the source exists;
2. the leaf is not a symbolic link;
3. its type matches the stored expected kind;
4. canonical resolution remains within the exact stored containment anchor;
5. the host path remains representable by the existing safe mount grammar.

For cache mounts it also rereads the generation manifest and requires the exact
key digest, plan digest, generation, and staging state captured at preparation.
Every path component below an already canonical anchor must remain a plain
non-symlink object; parent-component symlinks are rejected even if they resolve
back inside the anchor.

Repository bindings must resolve exactly to the prepared canonical repository.
Cache bindings must remain under the canonical managed cache root. Artifact
bindings must remain under the canonical run root.

Failure returns a typed runtime/workspace error. It must produce zero Docker
supervisor requests, no container creation, no receipt, and no cache promotion.
The error is not converted into a check FAIL or PASS.

The authoritative insertion point is the start of
`DockerCompatibleRuntime::execute_check`, before `DockerLifecyclePlan::build`.
The structured expectations travel in a nonserialized field of `DryRunCheck`,
which is already passed to that method. No Docker lifecycle path may invoke a
supervisor without first validating those expectations.

This check narrows but does not eliminate the operating-system TOCTOU window:
Docker receives a pathname rather than an open host file descriptor. Entry
locks prevent cooperative CCP mutators that honor the same lock; external
manual deletion remains unsupported and cannot be made safe by revalidation
alone.

### Managed cache pins for `guard exec`

The CLI contract is:

```console
commit-ci-preflight guard exec \
  --managed-cache-root /absolute/owned/cache-root \
  --managed-cache-source /absolute/owned/cache-root/entries/<key>/data \
  -- <program> [args...]
```

`--managed-cache-source` is repeatable and requires exactly one
`--managed-cache-root`. Supplying a root without a source is rejected to avoid
an unused, misleading declaration.

The parser accepts one root value globally. It canonicalizes and validates that
root before admission, canonicalizes each absolute source, rejects sources
outside that root, deduplicates canonical sources, and ignores caller ordering.

Before admission, CCP performs only bounded argument and root validation. Once
the guard owns the host-wide slot, it opens the already-owned cache without
initializing or repairing it. Sources are deduplicated and sorted by entry
directory before exclusive locks are acquired, preventing caller-order
deadlocks.

Every source must be exactly a completed entry's `data` directory beneath
`<root>/entries/<key>/data`. Staging directories, workspaces, incomplete
entries, symlinks, wrong object types, foreign roots, temporary roots, and path
escapes fail closed. CCP does not create any missing path.

One cache helper owns this interpretation. It opens the exact root through
`ManagedCache::open`, validates the ownership marker and fixed layout, requires
the exact `entries/<sha256-key>/data` shape with no alternate or extra
components, acquires the entry lock, then revalidates the key-shaped directory,
plain data directory, completion marker, and complete generation manifest. The
manifest key digest must match the directory key; its plan digest and generation
must be valid and self-consistent. Root validation uses the existing
`ResolvedCacheRoot` temporary-root and symlink-component predicates.

Opening performs read-only ownership/layout validation. Acquiring the advisory
pin may update only the existing lock file's bounded owner text; it does not
initialize directories or repair ownership/completion metadata.

Pins are acquired before the child starts, revalidated immediately before the
supervisor call, and held until child containment cleanup and guard-session
release are complete. Lock contention fails closed; it does not wait while
holding the heavy slot indefinitely and does not bypass admission.

The RAII pin collection is created after guard admission and before child
launch. It remains in the outer `print_guard_exec` scope until every success,
child failure, timeout, cancellation, forced-containment-cleanup, watchdog, and
session-release path has completed. Pin acquisition and revalidation failures
finish the guard session without starting a child.

The declaration does not prove that the child actually mounts or uses the
path. It prevents a declared CCP cache source from being changed by cooperative
CCP operations while the child may use it. It does not turn `guard exec` into a
receipt-producing or argv-attesting command.

## Cleanup and quarantine contract

Automatic deletion remains out of scope and `cache cleanup` stays preview-only.
The shared lock becomes the normative prerequisite for any future mutating
cleanup or quarantine implementation:

1. select a candidate without mutating it;
2. acquire its entry lock;
3. revalidate ownership, status, type, and exact path after acquisition;
4. abort on busy, changed, missing, malformed, or ambiguous state;
5. only then perform an owner-reviewed recoverable transition.

No implementation may delete from a stale inventory snapshot or treat absence
of a visible process as proof that a cache entry is unused.

Future mutators must use non-blocking locks and a documented order compatible
with promotion/recovery. They must never wait for an entry while retaining a
root promotion lock; busy state is a fail-closed result, not a wait or bypass.

## Failure, privacy, and compatibility

- Existing valid configurations and serialized normalized plan digests,
  receipts, policies, dry-run JSON, and verification behavior remain
  byte-compatible. Internal prerelease Rust structs may gain nonserialized
  fields required by this invariant.
- Serialized dry-run output gains no absolute containment-anchor fields.
- Pin errors may identify the local path only in human-readable local stderr.
  Absolute paths never enter structured JSON, journals, receipts, resource
  history, admission metadata, telemetry, or remote evidence.
- `guard exec` still emits no receipt and retains its existing child-exit,
  timeout, cancellation, cleanup, and resource-pressure classifications.
- Missing, symlinked, wrong-type, busy, foreign, or ambiguous managed sources
  fail before child execution.
- No cache root is initialized, repaired, pruned, quarantined, or deleted by
  the pin path.
- No new dependency, background process, network operation, Docker invocation,
  or persistent schema is introduced by the implementation tests.

## Deterministic TDD strategy

### Prepared-generation ownership

- clone a prepared entry;
- drop one clone and prove staging data and the lock remain live;
- prove a second preparation is still blocked;
- drop the final clone and prove only the matching staging generation is
  removed;
- preserve promotion and failed-generation regression coverage.

### Runtime mount revalidation

- valid prepared repository, cache, regular-file artifact, and directory
  artifact sources pass;
- missing source, leaf symlink, parent-component escape, and wrong source type
  fail;
- a recording `SupervisorPort` proves invalid input produces zero Docker
  requests and valid input reaches exactly the expected `docker create` call;
- dry-run remains deterministic, shell-free, non-executing, and serialization
  compatible.

### Guard pinning

- an exact completed entry pins successfully;
- duplicate declarations acquire only one lock;
- multiple declarations use deterministic order;
- active preparation and a second guard pin return busy;
- incomplete, staging, missing, symlinked, foreign, temporary, and escaped
  sources fail without child execution;
- a recording child seam proves the pin remains held through success, child
  failure, timeout/cancellation cleanup, and guard-session release;
- a deterministic hook removes or symlink-replaces the source after pin
  acquisition but before the supervisor call, proving revalidation fails,
  zero child requests occur, the guard session releases, and the pin unlocks;
- legacy `guard exec` without pin flags remains behaviorally unchanged.

### Documentation contract

Update `CACHE_AND_WORKSPACE.md`, `RUNTIME.md`, `LOCAL_RUN.md`,
`COORDINATION_RUNBOOK.md`, `TESTING_AND_FAULT_INJECTION.md`, and
`THREAT_MODEL.md`. Documentation must distinguish the standard `run` lock,
the opt-in guard pin, spawn-boundary revalidation, and unsupported manual
deletion.

## Delivery sequence

1. Shared prepared-generation final-drop ownership and focused cache tests.
2. Structured mount expectations plus spawn-boundary validation and recording
   runtime tests.
3. Opt-in `guard exec` managed-cache pin CLI and deterministic lifecycle tests.
4. Documentation, full non-heavy Rust checks, code review, and issue #4 update.

Each step is separately reviewable and committed only after its red-green TDD
cycle passes. Docker, OrbStack, a CCP heavy run, receipt publication, push, PR,
and merge remain separate later authorization boundaries.

## Acceptance criteria

1. No prepared-entry clone can remove a staging generation still owned by
   another clone.
2. A missing, symlinked, escaped, wrong-type, or generation-mismatched live
   mount source detected at the spawn boundary cannot cause a Docker supervisor
   request or produce evidence.
3. A declared completed cache entry used by `guard exec` remains locked from
   acquisition through child and cleanup completion.
4. Standard `run` and future cleanup use the same per-entry lock authority;
   no second lease or recovery authority exists.
5. Legacy unpinned `guard exec`, serialized configuration digests, receipts,
   policies, dry-run JSON, and valid historical fixtures remain compatible.
6. Tests are deterministic and require no Docker, network, model, CCP run, or
   host cache mutation.
7. Documentation states that undeclared raw paths and manual deletion remain
   outside the guarantee and must not be reinterpreted as safe.

Same-path replacement by a non-cooperative actor is not claimed detectable on
every supported platform. The entry lock prevents cooperative CCP replacement;
manual mutation that ignores the lock remains unsupported.

Issue #4 closure for this tranche proves cooperative CCP mutation is prevented
and changed sources detected by the final validator fail closed. It does not
identify the historical deleting actor or claim protection from arbitrary
external deletion or same-path replacement after validation.
