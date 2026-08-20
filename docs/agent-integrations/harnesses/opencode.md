# OpenCode reference

## Evidence state and scope

CCP level: **L1**. Upstream delivery is source-documented, but fresh local CCP
observation is absent. Source reviewed 2026-08-20: <https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only the documented OpenCode extension or plugin surface. CCP does not
install extensions or change user- or project-global settings.

## Bootstrap and discovery

Automatic context injection needs a unique fresh-session marker. Until then the
reference is manual-reference-only.

## Tool mapping

Use live native tools for files, shell commands, search, task tracking, and
delegation; do not assume another harness's names or permissions.

## CCP activity sequence

The [common contract](../HARNESS_INTEGRATION.md) applies without exception,
including host-wide admission ownership and terminal outer-result truthfulness.

## Fresh-session smoke protocol

Use a disposable project and no-op unique marker. No container, admission lock,
receipt, or source mutation is allowed in an L2 smoke.

## Failure and rollback

If discovery is not observed, retain manual L1 guidance and use GitHub-hosted
CI as the fail-closed path.

## Privacy and neutrality

Do not publish paths, settings, raw logs, prompts, credentials, or customer
data. CCP does not depend on OpenCode.
