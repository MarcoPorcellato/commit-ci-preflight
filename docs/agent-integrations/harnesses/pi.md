# Pi reference

## Evidence state and scope

CCP level: **L1**. Upstream delivery is source-documented but no local CCP
discovery or execution has been recorded. Source reviewed 2026-08-20:
<https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use only Pi's documented extension or plugin surface. CCP does not install or
modify global or user-specific configuration.

## Bootstrap and discovery

Any automatic context claim requires a fresh-session unique marker. Before that,
the page is a manual reference.

## Tool mapping

Use only native file, terminal, search, task, and delegation capabilities
actually present in the session.

## CCP activity sequence

Follow [HARNESS_INTEGRATION.md](../HARNESS_INTEGRATION.md): exact-head
preflight, single heavy owner, terminal handoff, and fallback.

## Fresh-session smoke protocol

Use a disposable no-op marker. Never start CCP, a container, or a receipt run
just to demonstrate discovery.

## Failure and rollback

If discovery is unavailable, retain L1 manual guidance and choose
GitHub-hosted CI for qualification.

## Privacy and neutrality

Keep paths, settings, prompts, logs, credentials, and customer data local. CCP
has no dependency on Pi.
