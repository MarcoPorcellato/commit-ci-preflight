# GitHub Copilot CLI reference

## Evidence state and scope

CCP level: **L1**. Upstream marketplace delivery is source-documented, but no
local CCP integration has been observed. Source reviewed 2026-08-20:
<https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only the harness's documented marketplace and plugin mechanism. CCP does
not add marketplaces, install plugins, or modify global configuration.

## Bootstrap and discovery

Any session bootstrap claim requires a unique fresh-session marker. Without it,
the reference remains manual-reference-only.

## Tool mapping

Use native capabilities available in the current CLI session for files, shell,
search, task tracking, and delegation.

## CCP activity sequence

Observe the [common contract](../HARNESS_INTEGRATION.md), especially exact-head
preflight, host-wide admission ownership, and the outer-result rule.

## Fresh-session smoke protocol

Run only a disposable unique-marker check. It must not start Docker, OrbStack,
CCP admission, or a receipt-producing command.

## Failure and rollback

If discovery is missing, retain L1 manual guidance and use GitHub-hosted CI as
the qualification fallback.

## Privacy and neutrality

Do not retain paths, settings, logs, prompts, credentials, or customer data in
public CCP evidence. CCP does not depend on this CLI.
