# Kimi Code reference

## Evidence state and scope

CCP level: **L1**. Upstream marketplace delivery is source-documented; it is
not a local discovery or execution result. Source reviewed 2026-08-20:
<https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only the harness-owned marketplace surface. CCP does not install a plugin,
write a profile, or alter global configuration.

## Bootstrap and discovery

Use a fresh-session marker before describing discovery as observed. Otherwise
this page remains manual-reference-only.

## Tool mapping

Map actions to the currently available Kimi Code file, command, search, task,
and delegation facilities only.

## CCP activity sequence

Follow [HARNESS_INTEGRATION.md](../HARNESS_INTEGRATION.md) for exact-head
preflight, one heavy owner, terminal handoff, and fallback.

## Fresh-session smoke protocol

Use a disposable no-op unique marker; do not start a container or receipt run.

## Failure and rollback

If not observed, keep L1 manual guidance and use GitHub-hosted CI for any
qualification requirement.

## Privacy and neutrality

Keep paths, settings, prompts, logs, credentials, and customer data local. CCP
has no runtime dependency on Kimi Code.
