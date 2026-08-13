# Local resource observation history

## Purpose

Commit CI Preflight protects a shared developer machine with deterministic
macOS admission thresholds and an in-run watchdog. Static thresholds are safe,
but they cannot distinguish a five-second documentation check from a long
containerized test suite. Local resource observation history is the first,
non-enforcing step toward workload-aware admission.

This tranche changes no admission threshold, watchdog threshold, cancellation
rule, receipt field, remote workflow, or exit status. It records bounded local
summaries only after a `guard exec` workload has passed the existing pre-start
gate. History write failures are advisory and never change the guarded
process result.

## Evolution record

| Policy/tranche | Behavior | Status |
|---|---|---|
| `macos-v1` | Fixed pre-start thresholds and two-second watchdog | Shipped in PR 12 |
| `macos-v2` | Swap-only admission relaxed to `min(10 GiB, 30% RAM)` | Shipped in PR 15 |
| observation history v1 | Per-profile baseline, extrema, duration and outcome; no prediction | Legacy file retained unchanged |
| observation history v2 | Adds bounded workload/executor context for comparable cross-repository samples | Current tranche |
| forecast shadow mode | Backtest a deterministic upper-bound forecast without changing admission | Future, requires sufficient comparable samples |
| workload-aware admission | May relax only the pre-start decision while retaining hard limits and watchdog | Future owner gate |

## Operator usage

Assign a stable workload class that contains no repository, customer or user
identifier:

```console
commit-ci-preflight guard exec \
  --resource-profile ready \
  --resource-workload-family brain-linux-ci-v1 \
  --resource-executor orbstack \
  --resource-cache-state warm \
  --resource-execution-mode emulated \
  --resource-target-platform linux-amd64 \
  --resource-cpu-limit-millis 2000 \
  --resource-memory-limit-bytes 8589934592 \
  -- make ci-linux-ready
```

Profiles, workload families and target labels contain 1–64 ASCII letters,
digits, hyphens or underscores. Profiles describe the check breadth, while a
workload family is a stable versioned cohort such as `brain-linux-ci-v1`.
Executor is one of `native`, `orbstack`, `docker`, `vm`, or `unknown`; cache
state is `cold`, `warm`, `mixed`, or `unknown`; execution mode is `native`,
`emulated`, or `unknown`. Requested limits are optional positive integers and
describe the runner ceiling, not measured consumption. Use
`--no-resource-history` when no local history should be retained. This switch
does not disable admission or the watchdog.

Direct argv beginning with `docker --context orbstack` or
`docker --context=orbstack` is classified automatically. Indirect scripts and
Make targets must pass explicit context because CCP intentionally does not
inspect script contents, environment values, runtime APIs, or container state.

Do not wrap a command that recursively invokes another CCP `run`, `benchmark`
or `guard exec`. Host-wide admission is intentionally non-reentrant: the inner
command waits for the outer command's slot. Run CCP's own full test suite
directly after a successful resource probe because its CLI integration tests
exercise guarded commands internally.

On macOS, history is stored at:

```text
~/Library/Application Support/commit-ci-preflight/resource-history-v2.jsonl
```

The directory is outside temporary storage and survives ordinary reboots. To
reset learning data, stop any CCP-guarded workload and remove only that exact
JSONL file. Do not remove the separate admission coordinator or build-cache
roots. CCP automatically retains at most the 500 newest v2 records. The legacy
`resource-history-v1.jsonl` is neither migrated nor deleted automatically.

Read the strict bounded v2 report without starting a workload or changing any
resource/admission state:

```console
commit-ci-preflight resource history --json
```

Malformed, oversized or unsafe history fails closed; the command never repairs
or truncates uncertain input.

Tests and managed automation may set `CCP_RESOURCE_HISTORY_DIR` to an explicit
absolute, non-symbolic directory. This is a storage-location override only; it
does not change policy, retention, schema or privacy validation. Relative,
temporary aliases containing symbolic components, and otherwise unsafe roots
are rejected and history becomes advisory-unavailable.

## Record contract

Each v2 JSONL record contains:

- schema and macOS policy version;
- a caller-supplied bounded profile and stable workload family;
- executor, cache state, execution mode and optional target platform;
- optional requested CPU millicores and memory bytes;
- Unix start time and elapsed milliseconds;
- `completed`, `failed`, `cancelled`, `timed_out`, or `resource_pressure`
  outcome, plus an optional hard/soft/probe watchdog-trip reason;
- sample count;
- baseline and minimum available-memory percentage;
- baseline and minimum reclaimable uncompressed bytes;
- baseline and maximum compressor bytes;
- baseline and maximum swap bytes;
- physical-memory bytes used as the denominator.

The first sample is the already validated pre-start snapshot. The watchdog
adds one sample every two seconds. Only extrema are retained; the complete
time series is not written.

History deliberately excludes commands, arguments, environment names or
values, repository names, paths, commit identifiers, usernames, hostnames,
container identifiers, output, and file contents. It is local operational
telemetry, never a receipt or attestation, and is never transmitted by CCP.

## Integrity and failure behavior

- The history root and file must be absolute, non-symbolic regular filesystem
  objects.
- Records use a strict schema and deterministic one-record-per-line JSON.
- Updates are written to a private create-new temporary file, synchronized,
  then atomically renamed.
- A malformed, oversized, symbolic or otherwise uncertain existing history is
  left untouched. CCP emits a generic warning and continues with the original
  guarded-workload result.
- V2 rotation is bounded to 500 records and eight MiB of existing input. The
  legacy v1 file keeps its 100-record/one-MiB contract.

The host-wide admission slot remains held while a record is finalized, so two
cooperating heavy workloads cannot race the history update.

## Forecast qualification gate

Observation history must not be treated as a prediction until a later change
implements and backtests a deterministic forecast. That gate should require:

1. at least 10 comparable successful samples for exploratory shadow results;
2. preferably 20 samples before any enforcement change;
3. a stable profile, container limit and cache-state classification;
4. an upper-bound estimate derived from recent peak deltas plus an explicit
   safety margin;
5. a predicted compressor peak below the 35% soft watchdog threshold with
   additional margin;
6. unchanged 45% hard compressor trip, other hard limits, fail-closed probes,
   cancellation and host-wide serialization;
7. fallback to the current fixed policy for insufficient, stale, mixed or
   contradictory history;
8. backtesting against both successful runs and known memory-pressure incident
   traces.

No LLM is needed or permitted in this decision path. Forecast inputs and the
resulting rule must remain deterministic, inspectable and reproducible.

## Current limitations

- Only macOS `guard exec` admitted workloads write observation records in this
  tranche. Official launcher coverage is cooperative; direct runtime commands
  that bypass CCP are invisible.
- Pre-start denials and probe failures are not recorded.
- `run` and `benchmark` do not yet write history.
- Container cgroup `memory.peak` and stage boundaries are not yet available to
  CCP. Cache state and requested limits are declarations by adopting launchers,
  not independently attested measurements.
- Linux and Windows resource enforcement remains `unsupported_not_enforced`.

These limitations prevent premature predictive claims while still collecting
the host extrema needed for an evidence-based next decision.
