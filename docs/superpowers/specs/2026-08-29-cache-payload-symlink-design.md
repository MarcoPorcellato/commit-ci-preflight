# Opaque cache-payload symbolic-link design

Status: approved by owner on 2026-08-29
Date: 2026-08-29  
Baseline: `820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc`  
Scope: make completed cache generations reusable when ordinary package-manager
payloads contain symbolic links, without weakening CCP's managed-root boundary

## Decision

Commit CI Preflight will distinguish two filesystem trust domains inside an
owned cache root:

1. the **control plane**, whose directories, manifests, markers, locks,
   journals, entry names, and ancestry remain strictly free of symbolic links;
2. the **payload plane**, consisting only of the contents below an already
   validated `entries/<key>/data` or `.staging-*/data` directory, where a
   symbolic link is an opaque payload object that CCP may inventory and copy
   but must never follow on the host.

CCP will preserve a payload link's stored target text exactly. It will not
canonicalize, stat, open, traverse, validate the existence of, or derive host
authority from that target. Relative, absolute, broken, and outside-root
targets therefore have the same host-side treatment: preserve the link object
without dereferencing it.

The implementation will also construct prepared-generation cleanup ownership
immediately after creating a staging directory and before any fallible clone or
copy. A failed reuse attempt must not leave an unowned `.staging-*` directory.

No configuration, receipt, policy, cache-key, generation-manifest, or journal
schema changes are introduced.

## Problem and evidence

The cache currently accepts and promotes ordinary Python environment and
package-manager output that contains symbolic links. A later generation tries
to reuse that completed entry by cloning or copying its `data` tree. Both reuse
paths call traversal code that rejects every symbolic link as if it were a
managed control object.

A controlled Matrix V2 execution demonstrated the contradiction:

- one completed virtual-environment payload contained four normal environment
  links;
- one completed package-manager payload contained 136 wheel, archive, and
  editable-install links;
- the preceding generation had accepted and promoted both payloads;
- the following generation failed during cache preparation with
  `symbolic link found inside managed cache root`;
- the failure happened before the new outer receipt could be written;
- the failed clone left an empty staging directory because cleanup ownership
  was created only after the fallible reuse step.

This is a producer lifecycle defect: CCP can create a complete cache state that
the same producer cannot consume. It is not evidence that the link targets were
followed, that the payload was malicious, or that a project check failed.

The baseline and current upstream source retain the same contradiction:

- `ManagedCache::prepare_entry` creates staging and then calls
  `try_clone_tree` or `copy_tree` before constructing
  `PreparedCacheGenerationOwner`;
- `try_clone_tree` first calls `bounded_tree_size`;
- `bounded_tree_size` rejects any link;
- `copy_tree` also rejects any link;
- promotion validates the payload root directory but does not recursively
  reject links below it.

## Goals

- Reuse normal cache payloads containing relative, absolute, broken, or
  outside-root symbolic-link targets without host dereference.
- Preserve the exact stored target bytes or platform-native target value when
  copying a link.
- Keep the owner marker, fixed root directories, entry directory, lock,
  completion marker, generation manifest, promotion journal, staging
  directory, payload-root directory, and every ancestor strictly link-free.
- Keep traversal bounded, deterministic, sorted, overflow-checked, and
  fail-closed for unexpected filesystem object types.
- Ensure any fallible preparation step is covered by the existing generation
  cleanup owner.
- Preserve current serialized plans, cache keys, receipts, policies, manifests,
  journals, exit-code classes, and public evidence fields.
- Prove the behavior with deterministic tests that need no Docker, network,
  CCP heavy slot, host cache, or real package registry.

## Non-goals

- No recursive trust or validation of package-manager cache contents.
- No claim that a cache payload is reproducible, safe to execute, or part of
  receipt evidence.
- No host-side resolution of a link to determine whether its target is inside
  the payload.
- No automatic cleanup, eviction, quarantine, repair, migration, or adoption
  of existing cache state.
- No weakening of cache-root, source-mount, guard-pin, ownership-marker, lock,
  manifest, journal, or promotion validation.
- No new cache schema or compatibility flag.
- No Windows payload-link support until native creation semantics can preserve
  an opaque target without guessing whether the link addresses a file or a
  directory.
- No producer installation, adopter retry, receipt publication, push, PR, or
  release as part of the implementation tranche.

## Trust boundary

### Control plane

The control plane remains link-free. Existing strict checks continue to apply
to:

- the selected cache-root path and every existing component;
- `.ccp-cache-root-v1.json`;
- `entries/` and `workspaces/`;
- `entries/sha256-<key>/`;
- `.entry-lock-v1`, `.complete-v1`, and `.generation-v1.json`;
- `.promotion-lock-v1` and `.promotion-journal-v1.json`;
- `.staging-*` and `.backup-*` generation directories;
- the `data` directory at the root of a complete, staging, or backup
  generation;
- any source path accepted by cache pinning or runtime mount validation.

A link at any of these positions remains `SymlinkInManagedRoot`. The change
must not turn the whole cache entry into a link-permitting recursive tree.

### Payload plane

Only descendants of a separately validated plain `data` directory enter the
payload plane. For each child object, traversal uses `symlink_metadata` and
dispatches by the object itself:

- regular file: count or copy the file;
- plain directory: descend in deterministic name order;
- symbolic link: count or recreate the link without following it;
- any other object type: fail with `UnexpectedEntry`.

Traversal never calls `canonicalize`, `metadata`, `File::open`, `fs::copy`, or
directory enumeration on a link path. The link target is read only with the
platform link-reading primitive and used only as the target argument to the
platform link-creation primitive at the destination.

An absolute, broken, recursive, or outside-root target is data. It neither
authorizes access to that host path nor changes the containment anchor. If a
later containerized project process follows the link, resolution happens in
the existing container mount namespace and remains subject to the current
runtime boundary. Cache payload bytes remain mutable, unattested performance
state.

## Component design

### Typed traversal policies

Replace the one ambiguous recursive helper with explicitly named policies:

- a strict managed-structure traversal that continues to reject every link;
- a payload traversal whose root must already be a validated plain `data`
  directory and whose descendants may contain opaque links.

The payload traversal is not a public API. Its caller must supply the validated
payload root rather than an arbitrary path, preventing accidental reuse for
the owner marker, entry root, workspace root, journal, or lock hierarchy.

The existing `MAX_INVENTORY_NODES` applies equally to files, directories, and
links. Each visited link consumes one node and one `files` unit. Its accounted
byte size is the byte length of its stored target on Unix. All totals remain
checked for overflow. Inventory output remains deterministically sorted and
retains its current schema; the documented `files` field continues to mean
non-directory payload objects, not only regular files.

Entry inventory retains its current accounting of control files and
directories, but switches to the payload policy only at these exact plain-root
shapes:

- `entries/<key>/data`;
- `entries/<key>/.staging-*/data`;
- `entries/<key>/.backup-*/data`.

Every other subtree remains under the strict traversal policy. The
implementation plan must enumerate the currently accepted entry-level objects
before changing inventory code so existing link-free totals and legacy
internal states are not silently reclassified.

### Clone and fallback-copy behavior

On macOS, `clonefile` remains an optimization. Before invoking it, CCP performs
the bounded payload traversal, which proves the source root is plain and every
descendant is a supported payload object without following links. A successful
clone must preserve link objects as links. Unsupported clone errors retain the
existing deterministic fallback.

The Unix fallback copy performs these steps for a link:

1. inspect it with `symlink_metadata`;
2. read the stored target with `read_link`;
3. ensure the destination path does not already exist as any object;
4. recreate it with `std::os::unix::fs::symlink`;
5. do not inspect or follow the target before or after creation.

Regular files continue through `fs::copy`; directories are created as plain
directories and traversed in sorted order. A partial destination is owned by
the prepared-generation cleanup object and is removed through that existing
validated lifecycle when preparation fails.

The clone path must not rely only on `clonefile`'s behavior. Tests must prove
the common traversal contract and the fallback-copy contract independently.

### Windows behavior

Windows remains fail-closed for payload links in this tranche. Creating a
Windows symbolic link through the standard library requires choosing file-link
or directory-link semantics; an opaque broken target does not provide a safe
portable way to infer that choice. The implementation must return a typed
unsupported payload-link error before copying or claiming reuse, while
preserving current strict control-plane errors.

Windows caches with no symbolic links remain behaviorally unchanged. Native
Windows link preservation may be designed later using reviewed reparse-point
semantics and native tests; macOS or Linux evidence must not be relabeled as
Windows evidence.

### Preparation ownership and cleanup

`PreparedCacheGenerationOwner` must be created immediately after both the
plain staging directory and its plain `data` root exist, and before removing
that empty root or attempting clone/copy reuse. It owns:

- the exact staging path;
- key digest, plan digest, and generation identity once available;
- the entry lock;
- cleanup eligibility for an unmanifested preparation phase and the existing
  manifest-validated staging phase.

The owner needs an internal phase that distinguishes:

1. **preparing**: exact owned staging path exists but no manifest has yet been
   committed;
2. **staging**: the exact matching staging manifest exists;
3. **promoted**: ownership no longer authorizes staging deletion.

On final drop, cleanup may remove only the exact owned `.staging-*` directory
while the entry lock is still held. In `preparing`, it validates the parent
entry identity, staging name, and plain staging root before removal. In
`staging`, it retains the current schema/key/plan/generation/state validation.
After promotion, it performs no deletion. It never scans for or removes other
staging directories.

Whole-generation removal must itself retain the no-follow boundary. On
supported Unix platforms, the selected recursive-removal primitive must be
documented and tested to unlink a payload link object rather than traversing
its target. A fixture places a sentinel outside the staging tree, links to it
from several payload depths, removes the owned staging generation, and proves
the sentinel and its descendants are byte-identical. A link replacing the
staging root is still rejected. Mutation by a non-cooperative actor during
removal remains outside the guarantee and is not mitigated by following links.

This ordering closes the observed leak without inventing a recovery action or
deleting historical residue. Existing unowned residue remains operator state.

### Promotion and recovery

Promotion continues to rename whole plain `data` directories atomically within
an entry. It does not walk or rewrite their payload descendants. Before a
staging generation can enter a promotion journal, CCP validates:

- the strict control-plane paths and matching manifest;
- the payload root as a plain directory;
- a bounded payload traversal that accepts only regular files, plain
  directories, and opaque links.

Recovery continues to reason about exact generation directories, marker bytes,
manifest bytes, and journal identity. It may rename or remove an owned whole
generation directory but must never resolve or act on a link target within its
payload. The generic removal helper remains strict for control-plane callers;
payload-aware deletion is not introduced by this tranche.

### Errors and evidence

Control-plane links retain the existing local message and exit classification.
Payload traversal adds typed distinctions for:

- unsupported payload-link preservation on the current platform;
- failure to read or recreate an opaque payload link;
- unsupported payload filesystem object.

The final exact names may be refined during TDD, but callers must not map a
preparation error to a project-check result. A preparation failure produces no
new receipt and no cache promotion. Human-readable local stderr may identify a
path under the local cache; receipts, journals, telemetry, and public evidence
must not gain absolute paths, target values, usernames, or payload contents.

## Deterministic TDD strategy

### Boundary classification

- reject a link at the cache root, owner marker, `entries`, entry directory,
  lock, marker, manifest, staging root, or payload-root position;
- accept links only one or more levels below a validated payload root;
- reject FIFO, socket, device, or other unsupported objects inside a payload;
- prove the node bound and size-overflow behavior remains fail-closed.

### Opaque traversal

On Unix, fixtures cover:

- a relative link to an existing sibling;
- a relative link to a missing sibling;
- an absolute link;
- a link whose target names a path outside the cache root;
- a self-referential and a mutually recursive link;
- nested links beneath sorted directories.

For every case, a test hook records filesystem operations and proves no target
stat, open, canonicalization, copy, or descent occurs. Inventory counts each
link once and does not count or size its target.

### Clone and copy

- fallback copy recreates the exact link target and leaves the target
  unresolved;
- ordinary files and directories preserve current behavior;
- a copied payload containing a link can be prepared and promoted;
- a second generation reuses that complete payload successfully;
- macOS clone preflight accepts the same fixture and the cloned destination
  retains link identity;
- forced clone fallback produces an equivalent payload tree;
- Windows-specific tests prove payload links fail before partial reuse while
  link-free payloads remain supported.

### Cleanup ownership

- inject failure before clone, during clone preflight, during fallback copy,
  and before manifest write;
- prove the exact owned staging directory is removed on final drop;
- prove the entry lock is held through cleanup and then released;
- prove unrelated and identity-mismatched staging directories are preserved;
- prove cleanup unlinks internal payload links without changing an external
  target sentinel;
- preserve clone-sharing, successful promotion, failed promotion, and
  promotion-recovery regression tests.

### Compatibility

- existing inventory JSON fixtures remain schema-compatible;
- link-free cache inventory totals remain unchanged;
- normalized configuration digests, cache keys, dry-run JSON, receipts,
  manifests, and promotion journals remain byte-compatible;
- no test requires Docker, network, a real package manager, the admission
  root, a host cache, or a CCP heavy command.

## Documentation and change surface

The implementation tranche will update together:

- `docs/CACHE_AND_WORKSPACE.md` with the control-plane/payload-plane boundary,
  inventory accounting, and platform status;
- `docs/THREAT_MODEL.md` with the no-follow invariant and container-resolution
  boundary;
- `docs/TESTING_AND_FAULT_INJECTION.md` with deterministic link and cleanup
  fault coverage;
- `CHANGELOG.md` with the user-visible cache-reuse correction;
- ADR 0006 from Proposed to Accepted after exact-document owner review.

Source changes are expected to remain concentrated in `src/cache.rs` and its
focused tests. If implementation requires a public CLI/config/schema change,
new dependency, broader filesystem authority, or edits outside the documented
surface, stop and return to design review.

## Delivery sequence

1. Commit this design and proposed ADR for exact owner review.
2. After approval, write a line-bound implementation plan using red-green TDD.
3. Implement typed payload traversal and accounting with focused tests.
4. Implement Unix link-preserving fallback copy and macOS clone qualification.
5. Move preparation cleanup ownership before fallible reuse and cover injected
   failures.
6. Reconcile promotion/recovery validation and update public documentation.
7. Run formatting, warnings-denied checks, strict Clippy, all-target tests, and
   an independent code/security review.
8. In a separately authorized isolated candidate installation, qualify two
   cache generations: generation N creates and promotes a link-bearing cache;
   generation N+1 reuses it successfully.
9. Only after qualification may the stable producer contract be reviewed for
   replacement. Any adopter retry, evidence publication, push, PR, merge, or
   release remains a separate gate.

## Rejected alternatives

### Disable or rotate persistent caches

Rejected. It avoids reuse, discards the intended credit-saving benefit, and
does not correct a producer that accepts state it later rejects.

### Delete the link-bearing cache and retry

Rejected. It mutates operator state, hides the lifecycle defect, and provides
no durable protection for the next package-manager generation.

### Follow only links that appear to remain inside the payload

Rejected. Canonicalizing a target introduces host traversal, breaks broken and
recursive links, creates TOCTOU ambiguity, and grants meaning to untrusted
payload text.

### Permit links everywhere below an entry directory

Rejected. That would weaken ownership, marker, manifest, journal, lock, and
promotion invariants and make the trusted control layout ambiguous.

### Rewrite links as regular files or resolved directories

Rejected. It changes package-manager semantics, can copy data outside the
payload, and makes cache reuse nondeterministic.

### Infer Windows link type from the target

Rejected. Broken or outside-root targets make inference unreliable and would
require following untrusted host paths. Native Windows support needs a separate
reviewed design.

## Acceptance criteria

1. A Unix cache payload containing relative, absolute, broken, external-target,
   and recursive links can be inventoried, prepared, copied, promoted, and
   reused without CCP following any target on the host.
2. A link at any control-plane or payload-root position still fails closed.
3. Payload inventory remains bounded and deterministic; every link consumes
   one node and one file count, and its byte count is its stored target length.
4. Failed reuse leaves no newly created unowned staging directory and releases
   the entry lock only after validated cleanup.
5. Promotion and recovery operate on whole owned generation directories and do
   not resolve payload link targets; staging cleanup leaves every external
   target sentinel untouched.
6. Windows remains explicitly fail-closed for link-bearing payload reuse; no
   cross-platform claim exceeds native evidence.
7. Existing configuration digests, cache keys, receipts, policies, manifests,
   journals, and link-free cache behavior remain compatible.
8. Focused and full non-heavy Rust gates pass at the exact implementation HEAD.
9. A separately authorized two-generation candidate qualification proves both
   link-bearing cache creation and subsequent reuse before any installed
   producer replacement.
