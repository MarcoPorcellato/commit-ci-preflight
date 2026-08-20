# Coding-harness compatibility matrix

## Status and method

This is a public evidence ledger, not an installation directory. It was
reviewed on 2026-08-20 against the upstream integration inventory and porting
guidance at <https://github.com/obra/superpowers> and
<https://github.com/obra/superpowers/blob/main/docs/porting-to-a-new-harness.md>.
The upstream delivery state and CCP evidence level are independent. The matrix
is authoritative over the historical planning snapshot when upstream support
changes.

| Harness | Page slug | Upstream delivery knowledge | CCP level | Current public statement |
| --- | --- | --- | --- | --- |
| Claude Code | `claude-code` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Antigravity | `antigravity` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Codex App | `codex-app` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Codex CLI | `codex-cli` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Cursor | `cursor` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Devin CLI | `devin-cli` | RESEARCH_REQUIRED | L0 | No current CCP integration claim |
| Factory Droid | `factory-droid` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Gemini CLI | `gemini-cli` | RETIRED_OR_RESEARCH_REQUIRED | L0 | No current CCP integration claim |
| GitHub Copilot CLI | `copilot-cli` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Grok Build CLI | `grok-build-cli` | RESEARCH_REQUIRED | L0 | No current CCP integration claim |
| Kimi Code | `kimi-code` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| OpenCode | `opencode` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Pi | `pi` | SOURCE_DOCUMENTED | L1 | Reference guidance only |
| Hermes Agent | `hermes-agent` | RESEARCH_REQUIRED | L0 | No current CCP integration claim |

L1 documents an upstream reference surface; it does not certify that a local
installation, bootstrap hook, activity, or CCP run was observed. L2 through L4
require a dated sanitized evidence record under `docs/agent-integrations/evidence/`.
No row currently reaches L2, L3, L4, or VERIFIED.
