# OrbStack and cross-repository telemetry coverage

## Purpose

This document records the local integration audit performed on 2026-08-13 and
defines how repositories should identify heavy CI workloads supervised by
Commit CI Preflight (CCP). The goal is comparable, privacy-minimized resource
history across repositories, not system-wide process surveillance.

Resource history is advisory local telemetry. It is not a CI receipt,
attestation, billing record, or proof that an unobserved process did not run.
It never changes admission, watchdog, cancellation, remote CI, or merge policy.

## Audited local repositories

The audit used committed launcher/configuration files and persistent receipt
locations. Temporary worktree copies were not counted as separate adopters.

| Repository | OrbStack CI launcher | CCP use | Current classification |
|---|---|---|---|
| Matryca-per-Delineat | Yes: repository-owned Linux parity launcher | Guarded launcher plus local receipts | V2 family `brain-linux-ci-v1`; first official adoption target |
| commit-ci-preflight | Runtime fixtures and self-qualification only | Native tool and internal guarded tests | Producer of the telemetry contract; its full suite must not be recursively guarded |
| Matryca-knowledge | No independent launcher found | Projected documentation references only | Not an OrbStack adopter |
| matryca-plumber | No committed launcher found | No committed OrbStack/CCP entry point found | Not an OrbStack adopter |
| logseq-matryca-parser | No committed launcher found | No committed OrbStack/CCP entry point found | Not an OrbStack adopter |
| Emergent-AI-TRIZ | No OrbStack launcher found | Uses CCP for bounded native laboratory commands | CCP adopter, not an OrbStack adopter |

At the audit date, Matryca-per-Delineat is the only repository with a complete
official OrbStack CI path. The inventory is a point-in-time fact, not an
allowlist: a new repository becomes covered by adopting the contract below.

## Coverage model

| Launch path | Executor classification | Resource history coverage |
|---|---|---|
| `guard exec -- docker --context orbstack ...` | Automatically `orbstack` | Covered |
| `guard exec` wrapping a script or Make target with explicit v2 flags | Caller declaration | Covered |
| CCP `run` or `benchmark` | Existing admission contract | Not yet written to resource history |
| Direct Docker/OrbStack process bypassing `guard exec` | Unknown to CCP | Not covered |
| Process started by another tool without CCP | Unknown to CCP | Not covered |

CCP does not install a daemon, replace the Docker CLI, consume global Docker
events, or inspect every host process. Those approaches would broaden trust,
privacy, lifecycle, and failure domains. The truthful guarantee is therefore:
every official launcher that uses `guard exec` is tracked; bypasses are not.

## V2 adoption contract

Every official indirect launcher should supply:

- `resource-profile`: check breadth, for example `static`, `unit`, or `ready`;
- `resource-workload-family`: stable versioned cohort, independent of a path or
  repository name, for example `brain-linux-ci-v1`;
- `resource-executor`: `native`, `orbstack`, `docker`, `vm`, or `unknown`;
- `resource-cache-state`: `cold`, `warm`, `mixed`, or `unknown`;
- `resource-execution-mode`: `native`, `emulated`, or `unknown`;
- `resource-target-platform`: optional bounded label such as `linux-amd64`;
- requested CPU millicores and memory bytes only when the launcher knows the
  exact enforced ceilings.

Unknown is preferable to an inferred or stale value. If a runtime override
changes a requested limit and the launcher cannot convert it exactly, omit the
numeric field. Context is validated before admission so invalid labels cannot
silently fragment the history.

The following never belongs in resource history: command or arguments,
environment names or values, repository names, filesystem paths, Git commits,
usernames, hostnames, container identifiers, logs, output, secrets, customer
identifiers, or file contents.

## Why these dimensions are sufficient now

- Profile separates breadth such as static checks from full readiness.
- Workload family prevents unrelated pipelines from sharing a forecast cohort.
- Executor distinguishes host-native work from container/VM overhead.
- Execution mode separates native target execution from emulation.
- Target platform avoids mixing Linux amd64 and other target behavior.
- Cache state captures the largest expected cold/warm variance without
  inspecting cache contents.
- Requested limits make constrained runs comparable when they are known.
- Baseline/extrema, duration, samples, result, and watchdog reason remain the
  measured v1 signals.

Stage timings, container peak memory, image digest, runtime version, and cache
byte counts may be useful later, but require independent collection and privacy
review. They are not invented or approximated in v2.

## Rollout and quality gates

1. Ship the backwards-separate v2 history file in CCP; leave v1 untouched.
2. Update each official launcher with explicit context and capability detection
   so older CCP binaries fail clearly or retain their documented fallback.
3. Collect at least ten comparable successful samples per exact context for
   exploratory shadow analysis, preferably twenty before any policy proposal.
4. Reject mixed, insufficient, stale, or contradictory cohorts from forecasts.
5. Keep all current hard admission/watchdog gates and owner approval for any
   threshold change.
6. Re-audit repository launchers whenever a new OrbStack CI entry point is
   added; documentation alone is not proof of adoption.

## Operator checks

The persistent macOS v2 file is:

```text
~/Library/Application Support/commit-ci-preflight/resource-history-v2.jsonl
```

Use `commit-ci-preflight resource history --json` for a strict read-only view;
do not build ad-hoc readers that silently skip malformed records.

It survives ordinary reboots. Stop CCP-guarded work before inspecting or
removing it. Cleanup must target only this exact file; admission coordinator,
build caches, receipts, and the legacy v1 history have separate lifecycles.

For schema, rotation, atomic-write, privacy, and forecast details, see
[RESOURCE_OBSERVATION_HISTORY.md](RESOURCE_OBSERVATION_HISTORY.md).
