# Codex CLI reference

## Evidence state and scope

CCP level: **L1**. Upstream marketplace guidance is source-documented, not
locally qualified. Source reviewed 2026-08-20: <https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use the CLI's own plugin or skill discovery surface. CCP does not install global
skills, edit profile files, or mutate a user's agent configuration.

## Bootstrap and discovery

Verify any claimed automatic skill visibility through a fresh-session marker;
otherwise label this page manual-reference-only.

## Tool mapping

Use the CLI's current native file, command, search, task, and subagent tools.
Permission boundaries remain controlled by the harness and operator.

## CCP activity sequence

The [common contract](../HARNESS_INTEGRATION.md) applies unchanged, including
exact-head confirmation and the host-wide heavy-work ownership handoff.

## Fresh-session smoke protocol

Use a disposable project and unique no-op marker. No admission, container,
receipt, or source mutation belongs in this smoke.

## Failure and rollback

If discovery is not observed, retain L1 manual guidance only and use
GitHub-hosted CI as the fail-closed path.

## Privacy and neutrality

Do not publish paths, settings, raw output, credentials, or repository data.
CCP has no runtime dependency on Codex CLI.
