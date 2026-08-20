# Cursor reference

## Evidence state and scope

CCP level: **L1**. Upstream marketplace delivery is source-documented, not a
CCP-native observation. Source reviewed 2026-08-20: <https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use Cursor's documented plugin marketplace. CCP never writes editor settings,
hooks, workspace state, or global instructions.

## Bootstrap and discovery

Any session-start or plugin discovery claim needs a fresh-session marker. Before
that proof this page is manual-reference-only.

## Tool mapping

Map required actions to available Cursor file, terminal, search, task, and
delegation capabilities without assuming cross-harness equivalence.

## CCP activity sequence

Follow [HARNESS_INTEGRATION.md](../HARNESS_INTEGRATION.md): exact-head check,
one heavy owner, terminal outer result, receipt verification, or fallback.

## Fresh-session smoke protocol

Use a unique no-op marker in a disposable project. It must not invoke CCP or
other heavy host resources.

## Failure and rollback

If discovery is absent, keep L1 reference guidance and use GitHub-hosted CI for
unqualified work.

## Privacy and neutrality

Keep workspace settings, paths, logs, prompts, credentials, and user data out
of public artifacts. CCP does not depend on Cursor.
