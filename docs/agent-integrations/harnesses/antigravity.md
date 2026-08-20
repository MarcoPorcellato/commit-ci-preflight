# Antigravity reference

## Evidence state and scope

CCP level: **L1**. Upstream plugin delivery is source-documented, not locally
observed. Source reviewed 2026-08-20: <https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only Antigravity's documented plugin surface. CCP does not create plugins,
hooks, profiles, or user-level configuration.

## Bootstrap and discovery

Session-start behaviour must be proven with a fresh-session marker before any
automatic-integration claim. This page is otherwise manual reference material.

## Tool mapping

Map file reads, edits, shell execution, task tracking, and delegation only to
the live harness capabilities available to the current activity.

## CCP activity sequence

Apply the [common contract](../HARNESS_INTEGRATION.md) without change: exact-head
preflight, one host-wide owner, complete outer result, and terminal handoff.

## Fresh-session smoke protocol

Use a disposable project and a unique marker. The smoke is no-op and must not
start a container, acquire admission, or emit a receipt.

## Failure and rollback

If the marker is unavailable, classify the integration as manual-reference-only
and retain GitHub-hosted CI as fallback.

## Privacy and neutrality

Keep settings, paths, prompts, logs, tokens, and customer data local. CCP does
not depend on Antigravity.
