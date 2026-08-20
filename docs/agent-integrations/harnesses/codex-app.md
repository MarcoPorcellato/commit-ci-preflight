# Codex App reference

## Evidence state and scope

CCP level: **L1**. An upstream marketplace surface is source-documented; no
fresh local installation is claimed. Source reviewed 2026-08-20:
<https://github.com/obra/superpowers>.

## Harness-owned installation surface

Use the Codex App plugin marketplace when an operator chooses to install an
integration. CCP never changes application-wide plugin state.

## Bootstrap and discovery

Native skill discovery and plugin loading require a fresh-session marker before
they can be called observed. Until then this is a manual reference.

## Tool mapping

Use the app's native file, terminal, browser, task, and delegated-activity
facilities according to its current permissions.

## CCP activity sequence

Apply [HARNESS_INTEGRATION.md](../HARNESS_INTEGRATION.md) before all mutations
and heavy runs; worktrees do not isolate host-wide admission.

## Fresh-session smoke protocol

Use a disposable project and a unique no-op marker. Do not launch CCP, Docker,
or OrbStack during L2 discovery evidence.

## Failure and rollback

An unavailable marker means manual-reference-only. Preserve the record and use
GitHub-hosted CI whenever local evidence remains PENDING.

## Privacy and neutrality

Never export private settings, paths, prompts, logs, credentials, or user data.
CCP remains independent from Codex App.
