# Claude Code reference

## Evidence state and scope

CCP level: **L1**. Upstream delivery is source-documented; this page is not a
local installation record. Source reviewed 2026-08-20:
<https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only Claude Code's documented marketplace or plugin surface. CCP does not
write global configuration, install a plugin, or supply an installation command.

## Bootstrap and discovery

Automatic discovery is an upstream property to be checked in a fresh session.
Until observed, treat this page as a manual reference.

## Tool mapping

Use the harness's native file, shell, search, task, and delegation facilities;
do not assume a tool name or capability from another harness.

## CCP activity sequence

Follow the common [CCP contract](../HARNESS_INTEGRATION.md): exact-head
read-only preflight, one heavy-work owner, terminal handoff, then fallback.

## Fresh-session smoke protocol

In a disposable project, ask for a unique no-op marker from this page. Record
only whether it is visible before a tool call; do not start CCP heavy work.

## Failure and rollback

If discovery is absent or ambiguous, mark L1 as manual-reference-only and use
GitHub-hosted CI for unqualified local work.

## Privacy and neutrality

Do not publish local paths, settings, prompts, logs, credentials, or customer
data. CCP has no dependency on this harness.
