# Persistent cache and workspace contract v1

## Status and scope

PR 04 introduced persistent cache-root resolution, atomic ownership,
content-addressed cache keys, bounded inventory, preview-only cleanup, and a
pure workspace mount plan. PR 05 prepares those paths and executes the plan;
automatic cache deletion remains intentionally unavailable.

The separation is intentional:

- `cache path`, `cache inventory`, `cache cleanup --dry-run`, and `dry-run` are
  read-only;
- `cache init` creates only the owned cache root and its fixed directory
  skeleton;
- `run` creates only declared managed paths, holds a generation lock, executes
  checks, and marks cache entries complete only after every check passes.

## Persistent root resolution

The first available source wins:

1. `--cache-dir <absolute-path>`;
2. `CCP_CACHE_DIR`;
3. the platform application-cache location.

Platform defaults are:

| Platform | Default |
|---|---|
| macOS | `$HOME/Library/Caches/commit-ci-preflight-build-v1` |
| Windows | `%LOCALAPPDATA%/commit-ci-preflight-build-v1` |
| Linux/Unix | `$XDG_CACHE_HOME/commit-ci-preflight-build-v1`, otherwise `$HOME/.cache/commit-ci-preflight-build-v1` |

The default therefore survives ordinary reboots. The resolver rejects system
temporary directories, filesystem roots, the repository and its descendants,
relative paths, unresolved variables or `~`, dot/parent components, invalid
UTF-8, and every existing symbolic-link component. It fails closed when no
safe persistent default exists.

Build-cache defaults use a versioned namespace separate from the host-wide
admission coordinator. Pre-release builds that used
`commit-ci-preflight/admission/` may leave that legacy directory in place. The
resolver never adopts, moves or deletes it; the versioned build-cache root is
initialized independently. An operator may still select a previously owned,
valid cache root explicitly with `--cache-dir` or `CCP_CACHE_DIR`.

Use a documented explicit root when operator policy requires a different disk:

```console
commit-ci-preflight cache path \
  --repository /absolute/path/to/repository \
  --cache-dir /absolute/persistent/path
```

`cache path` resolves and validates the path but does not create it.

## Ownership and initialization

```console
commit-ci-preflight cache init --repository .
```

Initialization writes an exact versioned ownership marker atomically:

```text
.ccp-cache-root-v1.json
```

It then creates only `entries/` and `workspaces/`. Repeated and concurrent
initialization converges on the same marker. Existing unowned content, an
invalid marker, a symlinked path, or an uncertain write fails closed. Temporary
marker files left by an interrupted initializer are recognized as internal
recovery state; arbitrary files are not adopted.

The marker authorizes Commit CI Preflight to manage only the internal layout.
It is not permission to recursively delete the root.

## Content-addressed entries

Each declared logical cache receives a SHA-256 key over canonical JSON that
includes:

- cache-key schema version;
- project identifier;
- normalized plan digest;
- pinned runtime image;
- logical cache ID.

The resulting directory name is `sha256-<64 lowercase hexadecimal digits>`.
Cache payloads live beneath `entries/<key>/data`. An entry is complete only
when its exact `.complete-v1` marker exists. Missing or malformed completion
markers are reported as incomplete; they are never silently promoted.

## Inventory and disk budget

```console
commit-ci-preflight cache inventory --json
```

Inventory validates ownership again, rejects symlinks and unexpected entry
names, walks at most 100,000 nodes, and emits a deterministically sorted report.
The default reporting budget is 20 GiB and can be overridden with
`--disk-budget-bytes`. Exceeding the budget is reported; it does not trigger
automatic eviction.

## Schema 1.2 capacity preflight

The inventory budget remains reporting-only. A schema `1.2` `[storage]` policy
is separate: immediately before a local run begins its Git/runtime/workspace
work, CCP probes free bytes on the selected CCP-owned cache-root filesystem.
It requires the declared retained-free reserve, receipt/journal reserve,
maximum cache growth, and the sum of declared artifact bounds. Insufficient or
unreadable capacity is a fail-closed preflight error; it does not choose files
for deletion and it does not mutate cache generations. See
[`CONFIGURATION.md`](CONFIGURATION.md#storage-capacity-policy) for the exact
contract and digest boundary.

## Cleanup safety

```console
commit-ci-preflight cache cleanup --dry-run --json
```

PR 04 cleanup is preview-only. Omitting `--dry-run` is a usage error, and the
reported `deletion_performed` field is always `false`. Only incomplete entries
are listed as candidates. Complete cache entries are not selected merely
because the budget is exceeded, because the current schema has no trustworthy
age or least-recently-used evidence.

To remove data manually, first record `cache path` and `cache inventory`, stop
all runs using that root, and act only on exact operator-reviewed paths. The
tool itself does not yet perform deletion.

## Workspace isolation

The pure workspace planner produces explicit bindings:

| Purpose | Container target | Access |
|---|---|---|
| Repository | `/workspace` | read-only |
| Declared cache | `/workspace/<cache.mount_path>` | read-write |
| Declared artifact | `/workspace/<artifact-path>` | read-write |

Every writable host source is under the managed cache root. Duplicate targets,
path escapes, commas, control characters, and non-UTF-8 host paths are rejected
before Docker argv is rendered. The runtime inserts no shell, privileged mode,
Docker socket mount, or undeclared writable repository binding.

Because the repository binding is read-only, each nested destination must
already exist in the checkout: a directory for a cache and a file for an
artifact. The destination may be Git-ignored. Live preparation rejects a
missing destination, a symlink, a wrong object type, or a resolved escape
before creating the run workspace or invoking Docker. It never creates these
placeholders in the source checkout.

The read-only repository plus nested writable bindings is a containment
contract, not a complete sandbox against hostile code. The live runtime must
revalidate and prepare these exact paths before every execution.

Each plan digest has a `.run-lock-v1` file while a runner owns its workspace.
Normal completion removes it. A crash may leave the lock behind; the next run
fails closed and reports its exact path. The tool never guesses that such a lock
is stale. An operator may remove only that file after independently verifying
that no runner using the same cache root is active.

Cache payloads are mutable performance state. Their bytes are neither copied
into receipts nor claimed as reproducibility evidence. The immutable image,
normalized plan, commit, command results, and output digests are the attested
surface.

## Privacy and evidence limits

Local `dry-run` output includes absolute host paths so the operator can inspect
the exact bindings. Receipts and telemetry must not include those paths,
usernames, environment values, or cache contents.

Current evidence proves deterministic source behavior and macOS execution of
the test suite. Windows and Linux native cache/path behavior remains PENDING
until it is executed on those platforms; no macOS result is relabeled as native
evidence for another platform.
