# Multi-harness CCP integration design

Status: draft design for review
Date: 2026-08-20
Scope: Commit CI Preflight documentation and reusable adoption templates

## 1. Problem and outcome

Commit CI Preflight already has a repository-level adoption guide and a
cross-activity coordination contract. Agent activities still need one canonical,
current reference that explains how to apply that contract from different coding
harnesses without copying vendor-specific assumptions into the CCP runtime or
editing a user's private configuration.

The outcome is a documented integration contract and a compatibility matrix for
all harnesses currently documented as compatible with the Superpowers model.
The result is a reference and evidence system, not a new plugin framework.

## 2. Design principles

The design follows the separation used by Superpowers while keeping CCP's own
scope and licensing boundary:

1. Common contract. CCP adoption rules, exact-head receipts, admission
   ownership, handoff, fail-closed recovery, and GitHub fallback are identical
   for every harness.
2. Thin harness mapping. Each harness documents how its native actions map to
   the common contract: read, edit, shell, search, web, subagent, task
   tracking, and skill/instruction loading.
3. Install-owned delivery. Documentation may describe an install mechanism, but
   CCP never edits ~/.codex, ~/.claude, project-global configuration, or shell
   startup files on behalf of an operator.
4. Evidence before claims. A harness is VERIFIED only after a native smoke test
   proves that the referenced instructions are available in a fresh session and
   that the documented CCP handoff can be followed.
5. Vendor neutrality. CCP contains no dependency on Superpowers, Codex, Claude
   Code, GitNexus, Serena, or another agent vendor. Vendor names may appear only
   in integration references and compatibility evidence.
6. No hidden execution. The reference layer never executes a workflow, starts
   CCP, changes a lock, or publishes evidence merely because an agent read the
   documentation.

## 3. Proposed repository structure

    docs/
      HARNESS_INTEGRATION.md                 common CCP contract
      integrations/
        COMPATIBILITY_MATRIX.md              support/evidence ledger
        claude-code.md                       Claude Code reference
        codex-app.md                         Codex App reference
        codex-cli.md                         Codex CLI reference
        cursor.md                            Cursor reference
        antigravity.md                       Antigravity reference
        factory-droid.md                     Factory Droid reference
        copilot-cli.md                       GitHub Copilot CLI reference
        kimi-code.md                         Kimi Code reference
        opencode.md                          OpenCode reference
        pi.md                                Pi reference
        gemini.md                            Gemini instructions-file reference
    examples/
      agent/
        CCP_ACTIVITY_HANDOFF.md              copy/paste activity contract

The common document is the source of truth for CCP behavior. Per-harness pages
must not redefine receipt validity, admission safety, or recovery semantics;
they only explain delivery, tool mapping, and evidence requirements.

## 4. Common contract

Every harness page must link to and preserve these rules.

### Before a heavy run

    commit-ci-preflight --version
    git status --short --branch
    git rev-parse HEAD
    commit-ci-preflight resource status --json
    commit-ci-preflight admission status --json
    docker context show
    docker ps -q

The activity proceeds only after a fresh, coherent admit decision, readable
inactive admission with an empty queue, a responsive runtime, and an exact
source worktree. A process absent from one terminal is not proof that the
host-wide slot is free.

### During and after a run

- One owner activity controls the complete heavy lifecycle.
- Different worktrees do not create separate CCP admission slots.
- Slot ownership and queue ownership are interpreted separately.
- Unknown, missing, malformed, legacy, or contradictory lease state blocks.
- No activity manually deletes or quarantines locks, leases, tickets, or the
  admission root.
- An outer guard failure, timeout, cancellation, pressure stop, internal error,
  or uncertain cleanup never becomes PASS because an inner check was green.
- An exact-head receipt is qualified only when its complete outer contract and
  trusted verification pass.
- GitHub-hosted CI remains the fail-closed fallback when local evidence is not
  qualified.

### Public/private boundary

The public reference may contain harness names, commands, version dates, and
links to official documentation. It must not contain secrets, personal paths,
raw logs, customer data, model prompts, container identifiers, or private
GitNexus/Serena configuration. Local operator records may contain only the
bounded fields allowed by the coordination runbook.

## 5. Compatibility matrix contract

The matrix records facts, not aspirations. Each row contains:

| Field | Meaning |
|---|---|
| harness | Stable display name and canonical documentation slug |
| integration_shape | shell-hook, in-process-plugin, instructions-file, or native-skill-discovery |
| delivery_mechanism | Harness-owned installer, marketplace, extension, or bundled convention |
| bootstrap_surface | Where session-start or always-on context is delivered |
| tool_mapping | Where the action vocabulary is mapped to native tools |
| subagent_support | verified, partial, unverified, or not-applicable |
| skill_discovery | How the harness locates the reference instructions |
| ccp_entrypoint | Documented command boundary; never an implicit launcher |
| evidence_state | VERIFIED, SOURCE_DOCUMENTED, PARTIAL, UNVERIFIED, or NOT_APPLICABLE |
| verified_at | UTC date of the last evidence, or never |
| notes | Limitations, version constraints, and rollback path |

SOURCE_DOCUMENTED means an official or upstream reference describes the surface
but this repository has not executed a native smoke test. It must never be
presented as runtime qualification.

## 6. Harness coverage to prepare

The first matrix revision covers the currently documented Superpowers targets:

| Harness | Initial reference shape | Initial status policy |
|---|---|---|
| Claude Code | shell-hook/plugin | SOURCE_DOCUMENTED until native smoke evidence |
| Codex App | native skill/plugin discovery | SOURCE_DOCUMENTED until native smoke evidence |
| Codex CLI | native skill discovery | SOURCE_DOCUMENTED until native smoke evidence |
| Cursor | shell-hook/plugin | SOURCE_DOCUMENTED until native smoke evidence |
| Antigravity | plugin or installed context file | UNVERIFIED until installer behavior is proven |
| Factory Droid | harness-owned plugin marketplace | SOURCE_DOCUMENTED until native smoke evidence |
| GitHub Copilot CLI | shell-hook/extension | SOURCE_DOCUMENTED until native smoke evidence |
| Kimi Code | harness-owned marketplace | SOURCE_DOCUMENTED until native smoke evidence |
| OpenCode | in-process plugin | SOURCE_DOCUMENTED until native smoke evidence |
| Pi | in-process extension | SOURCE_DOCUMENTED until native smoke evidence |
| Gemini | extension-declared instructions file | SOURCE_DOCUMENTED until native smoke evidence |

The status is intentionally conservative. A page may provide useful operator
instructions while its evidence state remains unverified.

## 7. Per-harness page contract

Each page must contain the same headings:

1. Scope and evidence state — what is known and what is not.
2. Official installation surface — link and version/date policy.
3. Bootstrap/discovery mechanism — how the harness receives instructions.
4. Tool mapping — native names for file reads, edits, shell, search, web,
   subagents, task tracking, and skill invocation where applicable.
5. CCP activity sequence — preflight, one owner, receipt/handoff, fallback.
6. Fresh-session smoke test — a bounded test with a unique marker; no heavy CCP
   run is implied.
7. Failure and rollback — what to preserve and how to stop without touching
   another activity's state.
8. Privacy and vendor-neutrality notes — what must stay local/private.

If a harness has no automatic session-start or always-on instruction surface,
the page must say NOT_SUPPORTED_AS_AUTOMATIC_BOOTSTRAP rather than suggesting
that a pasted prompt is equivalent. A manual activity handoff remains useful,
but it is not a verified harness integration.

## 8. Reusable activity handoff template

examples/agent/CCP_ACTIVITY_HANDOFF.md will be a short copy/paste message that
tells an activity to:

- read the common contract and the relevant harness page;
- report exact repository, worktree, branch, base, and source SHA;
- use read-only discovery before mutation;
- run fresh CCP resource/admission/runtime checks before heavy work;
- reserve one host-wide slot and send a terminal handoff;
- preserve receipts and classify PASS, PENDING, or NOT_RUN truthfully;
- use GitHub fallback when local qualification is unavailable;
- keep vendor-specific tools local and out of public CCP artifacts.

The template is a convenience layer. It never overrides the common contract or
the trusted repository policy.

## 9. Testing and evidence

The documentation tranche will add deterministic repository tests that verify:

- every matrix row has a corresponding page;
- every page contains all required headings and the exact CCP preflight command
  family;
- every page states its evidence state and verification date;
- no page claims VERIFIED without a bounded evidence reference;
- the handoff template contains exact-head, ownership, fail-closed, and GitHub
  fallback rules;
- public docs contain no personal absolute paths, secrets, raw logs, or private
  tool configuration.

Native harness smoke tests are separate and opt-in. They require the relevant
harness installed, a clean disposable project, and an evidence transcript that
does not include private prompts or user data. They must never be faked from a
documentation test.

## 10. Rollout and rollback

1. Specification: review this design and freeze the common contract.
2. Reference docs: add the common page, matrix, harness pages, and handoff
   template.
3. Static contract tests: validate structure, links, privacy boundaries, and
   evidence-state claims.
4. Native smoke evidence: qualify one harness at a time when its real
   environment is available.
5. Maintenance: refresh only the affected harness row/page when upstream
   installation or tool surfaces change.

Rollback is documentation-only: remove or mark a stale harness row UNVERIFIED,
preserve the evidence history, and keep the common CCP contract and GitHub
fallback unchanged. No user configuration or product runtime needs to be
reverted.

## 11. Definition of done

- The common contract is readable without knowing any harness.
- All target harnesses have a page and a conservative matrix state.
- The activity handoff is copy/paste-ready and vendor-neutral.
- Static tests pass with no heavy CCP/OrbStack run.
- Every VERIFIED claim points to exact evidence and date.
- No installer edits a user's global files.
- The documentation remains compatible with the existing adoption and
  cross-activity runbooks.

## 12. Upstream references

- Superpowers repository and current harness list:
  https://github.com/obra/superpowers
- Superpowers new-harness invariants and acceptance contract:
  https://github.com/obra/superpowers/blob/main/docs/porting-to-a-new-harness.md
- Superpowers Codex reference:
  https://github.com/obra/superpowers/blob/main/docs/README.codex.md
- Superpowers Claude Code plugin reference:
  https://github.com/obra/superpowers/tree/main/.claude-plugin

These links are research references, not runtime dependencies and not copied
code.
