# CCP activity handoff

Copy this template between cooperating activities. Replace bracketed fields
with bounded operational facts. Do not paste raw logs, private paths,
environment values, secrets, user data, or container identifiers.

## Before a mutation

- Repository and remote identity: `[value]`
- Worktree and branch: `[value]`
- Base SHA and exact source SHA: `[value]`
- Worktree state and intended file allowlist: `[value]`
- Relevant CCP contract and harness reference read: `[value]`

## Before heavy work

- `resource status --json`: `[admit / deny / unknown with timestamp]`
- `admission status --json`: `[inactive / active / queue with lease state]`
- Runtime responsiveness: `[docker context and empty/non-empty state]`
- Heavy-slot owner: `[this activity / another activity / unknown]`
- Planned command: `[bounded command class, not raw arguments]`

Do not start if resource or admission state is unknown, contradictory, active
under another owner, queued unexpectedly, or runtime is unresponsive. The
absence of a local process is not evidence that the global slot is free.

## Terminal handoff

- Exact source SHA: `[value]`
- Outer guard outcome and exit code: `[value]`
- Receipt state and independent verification: `[PASS / PENDING / NOT_RUN]`
- Cleanup and runtime absence state: `[value]`
- Post-run admission state: `[value]`
- Follow-up or fallback: `[none / GitHub fallback / owner decision required]`

Only a complete exact-source SHA record with a terminal outer result and
independent receipt verification is eligible for PASS. When it is not eligible,
use the GitHub fallback rather than asserting local qualification.
