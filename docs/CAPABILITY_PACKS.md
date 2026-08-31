# Capability packs

Status: schema and inert library inspection/expansion only; no official pack execution in M2.

## TOML schema 1.0

Manifests use `schema_version = "1.0"` and strict TOML fields. A manifest is at
most 1 MiB; it has one identity (`pack_id`, `pack_version`, `license`, and
`description`), upstream sources, and 1–32 profiles. Each profile has a unique
identifier, bounded metadata, tools (at most 32), inputs (at most 64), hosts,
targets, runtime, environment, caches, storage, and checks. Lists are bounded
to the limits enforced by the library; paths and argv are validated as safe,
shell-free values.

## Identity and versions

The pack identity tuple is `(pack_id, pack_version, license, pack_digest)`.
Versions are immutable: publishing a changed manifest requires a new version
and therefore a new digest.

## Images and provenance

Runtime images must be digest-pinned. Tools and upstream inputs carry explicit
SHA-256-style provenance digests and URLs; an unpinned or missing provenance
value is rejected.

## Licensing

License values use SPDX-style identifier syntax. Syntax validation is not legal
review, and operators remain responsible for licensing and attribution review.

## Integrity and freshness

Integrity checks prove that declared bytes match their digest. Database and
rules freshness is separate: inputs may declare a creation timestamp and a
maximum age, but a valid digest does not make stale data fresh.

## Profile binding and expansion

Consumers bind a project, profile identifier, and receipt configuration
explicitly. Expansion resolves exactly one profile into exactly one normalized
execution plan, preserving the pack digest and evidence class.

## Expansion is not execution

Expansion only constructs an inspectable plan. It does not acquire resources,
start a runtime, run checks, create receipts, or qualify a result. No CLI
command exists for official pack execution yet.

## Preparation and I/O boundaries

Network access is disabled. Any required preparation is external to this
library. Sources are read-only; outputs must be explicit paths and bounded by
the existing plan contracts.

## Evidence classes

`deterministic` evidence should repeat exactly. `schedule-sensitive` evidence
can vary with scheduling or resource timing. `bounded-nondeterministic`
evidence may vary within documented bounds. The class describes evidence; it
does not qualify an execution.

## Non-goals

M2 adds no workflow DSL, package manager, tool installer, report interpreter,
receipt extension, publication mechanism, or policy override.

## M3 entry criteria

M3 may propose the `rust-deep` reference pack only after a reviewed manifest,
digest-pinned image and provenance, explicit offline preparation, bounded
inputs/outputs, profile-to-plan tests, freshness and integrity evidence, and a
separate execution/qualification decision. No such official pack is executable
from this M2 contract.
