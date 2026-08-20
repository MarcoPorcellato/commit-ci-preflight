# Multi-harness CCP integration design

Status: draft design for review
Date: 2026-08-20
Scope: Commit CI Preflight documentation, reusable adoption templates, and
evidence rules for coding harnesses

## 1. Decision summary

Commit CI Preflight will publish a vendor-neutral integration reference for the
coding harnesses currently listed by Superpowers. The reference will help an
agent activity apply the CCP contract correctly; it will not install, configure,
or embed Superpowers, Codex, Claude Code, or any other harness.

The design deliberately separates three things that are often conflated:

1. A harness is listed by an upstream project.
2. The CCP reference documents how that harness should behave.
3. A real installation has passed a CCP-specific fresh-session smoke test.

Only the third condition permits a CCP native-integration claim.

## 2. Goals and non-goals

### Goals

- One CCP contract shared by all harnesses.
- One compact reference page and a truthful evidence row per harness.
- A copy/paste handoff template usable by human operators and agent activities.
- Clear protection against overlapping host-wide CCP workloads.
- Stable, reviewable documentation that does not require live network access at
  runtime.
- Conservative rollout: source documentation first, native evidence later.

### Non-goals

- Shipping a CCP plugin, hook, extension, marketplace package, or installer.
- Editing private global harness configuration, startup files, or skill folders.
- Changing CCP runtime behavior, resource thresholds, receipts, policies, or
  GitHub gates.
- Treating a pasted prompt as an automatic bootstrap integration.
- Claiming that an upstream harness listing qualifies a local CCP run.

## 3. Design principles

1. **Common contract.** Exact-head receipts, admission ownership, handoff,
   fail-closed recovery, and GitHub fallback are invariant across harnesses.
2. **Thin mapping.** Per-harness pages map native actions to the common
   contract; they never redefine CCP semantics.
3. **Install-owned delivery.** Only a harness-owned installation mechanism may
   install its own integration artifacts. CCP documentation never mutates a
   user's global configuration.
4. **Evidence before labels.** Claims use a staged evidence model defined in
   section 6, not informal descriptions such as “supported”.
5. **Vendor neutrality.** CCP has no dependency on an agent vendor. Harness
   names appear only where they identify an integration reference or evidence.
6. **Privacy by default.** Public material excludes secrets, home paths, raw
   logs, customer data, model prompts, container identifiers, and private
   code-intelligence configuration.
7. **No hidden execution.** Reading an integration page never runs CCP, starts
   Docker or OrbStack, changes a lock, or publishes evidence.

## 4. Proposed repository structure

    docs/
      agent-integrations/
        HARNESS_INTEGRATION.md              common CCP contract
        COMPATIBILITY_MATRIX.md             current evidence ledger
        evidence/                           reviewed public, sanitized records
        harnesses/
          claude-code.md
          codex-app.md
          codex-cli.md
          cursor.md
          antigravity.md
          devin-cli.md
          factory-droid.md
          gemini-cli.md
          copilot-cli.md
          grok-build-cli.md
          kimi-code.md
          opencode.md
          pi.md
          hermes-agent.md
    examples/
      agent/
        CCP_ACTIVITY_HANDOFF.md             copy/paste activity contract

The common contract is normative. A harness page may add invocation guidance,
but cannot weaken receipt validity, admission safety, ownership, or recovery.

## 5. Common CCP contract

Every harness page and the handoff template must preserve the following.

### 5.1 Read-only preflight

    commit-ci-preflight --version
    git status --short --branch
    git rev-parse HEAD
    commit-ci-preflight resource status --json
    commit-ci-preflight admission status --json
    docker context show
    docker ps -q

Proceed only when the source worktree and SHA are known, resource status is a
fresh coherent admit decision, admission is readable with active false and an
empty queue, the configured runtime is responsive, and no competing owner is
known. A process absent from one terminal is not proof that the host-wide slot
is free.

### 5.2 Heavy-work ownership

- One activity owns the whole heavy lifecycle.
- Different worktrees isolate Git state only; they share CCP admission.
- Slot ownership and queue bookkeeping are interpreted separately.
- Unknown, missing, malformed, legacy, or contradictory lease information is
  blocking.
- No activity deletes, quarantines, or rewrites locks, leases, tickets,
  counters, or the admission root.
- The owner issues a terminal handoff with exact SHA, command result, receipt
  state, cleanup state, post-run admission, and runtime state.

### 5.3 Receipt and fallback truthfulness

- An outer guard failure, timeout, cancellation, pressure stop, internal error,
  or uncertain cleanup is not PASS even if an inner stage was green.
- A receipt is eligible only for its exact source SHA and trusted policy.
- SKIPPED, PENDING, stale-head, historical, incomplete, or unverifiable
  evidence is not PASS.
- GitHub-hosted CI remains the fail-closed fallback when local qualification is
  unavailable.

### 5.4 Harness-boundary rule

A CCP reference can explain how to use a harness. It does not prove that the
harness auto-loads the reference. Automatic bootstrap is a separate,
harness-native property and must be tested in a fresh session.

## 6. Evidence model

The matrix records two independent dimensions.

| Dimension | Values | Meaning |
|---|---|---|
| Upstream delivery knowledge | UNKNOWN, SOURCE_DOCUMENTED, OBSERVED | Whether a current official/upstream source or local observation describes the harness installation and bootstrap surface |
| CCP evidence level | L0, L1, L2, L3, L4 | The strongest CCP-specific evidence held for that harness |

CCP evidence levels are:

| Level | Required evidence | Claim allowed |
|---|---|---|
| L0 | Inventory row only | No CCP integration claim |
| L1 | Reviewed upstream source URL and retrieval date | Reference guidance exists |
| L2 | Fresh-session, no-op marker proves the reference is visible | Discovery/bootstrap observed |
| L3 | Bounded non-heavy activity follows the handoff and read-only preflight | Activity contract observed |
| L4 | Exact clean source SHA completes an approved CCP run with a complete outer result and verified receipt | Native CCP flow observed |

A row may be SOURCE_DOCUMENTED plus L1. It may not say VERIFIED. The word
VERIFIED is reserved for L4 and requires a sanitized, exact evidence reference
and UTC date. A harness that cannot automatically load a bundled reference is
recorded as MANUAL_REFERENCE_ONLY, not as an automatic integration.

## 7. Initial harness inventory

The inventory below is an upstream coverage snapshot, not a claim that CCP has
tested these environments. It tracks the current Superpowers README list as of
2026-08-20.

| Harness | Initial upstream knowledge | Initial CCP level |
|---|---|---|
| Claude Code | SOURCE_DOCUMENTED | L1 |
| Antigravity | SOURCE_DOCUMENTED | L1 |
| Codex App | SOURCE_DOCUMENTED | L1 |
| Codex CLI | SOURCE_DOCUMENTED | L1 |
| Cursor | SOURCE_DOCUMENTED | L1 |
| Devin CLI | SOURCE_DOCUMENTED | L1 |
| Factory Droid | SOURCE_DOCUMENTED | L1 |
| Gemini CLI | SOURCE_DOCUMENTED | L1 |
| GitHub Copilot CLI | SOURCE_DOCUMENTED | L1 |
| Grok Build CLI | SOURCE_DOCUMENTED | L1 |
| Kimi Code | SOURCE_DOCUMENTED | L1 |
| OpenCode | SOURCE_DOCUMENTED | L1 |
| Pi | SOURCE_DOCUMENTED | L1 |
| Hermes Agent | SOURCE_DOCUMENTED | L1 |

The first L2/L3/L4 targets are Codex App or CLI and Claude Code, because they
are the intended local operator environments. Other harnesses remain L1 until
their real installation surface is available and a bounded smoke test is
authorized.

## 8. Per-harness page contract

Each harness page must contain these headings:

1. **Evidence state and scope** — source URLs, retrieval date, current level,
   and explicit unsupported boundaries.
2. **Harness-owned installation surface** — marketplace, plugin, extension, or
   documented discovery convention; never a mutation recipe for user files.
3. **Bootstrap and discovery** — whether automatic session-start availability is
   source-documented, observed, or unavailable.
4. **Tool mapping** — native capability names or capability classes for file
   read/edit, shell, search, web, subagents, task tracking, and instruction
   loading.
5. **CCP activity sequence** — common preflight, one owner, receipt/handoff,
   and fallback.
6. **Fresh-session smoke protocol** — unique marker, disposable project, no
   heavy CCP invocation, expected observation, and sanitization rules.
7. **Failure and rollback** — how to preserve evidence and stop safely.
8. **Privacy and neutrality** — what remains local and what must not be
   published.

If a source does not establish automatic bootstrap, the page must use
MANUAL_REFERENCE_ONLY. A manual handoff can still be useful, but it is not an
automatic harness integration.

## 9. Reusable activity handoff

examples/agent/CCP_ACTIVITY_HANDOFF.md will instruct every activity to:

- read the common contract and the relevant harness page;
- report repository, worktree, branch, base, and exact source SHA;
- perform read-only discovery before any mutation;
- repeat resource, admission, and runtime checks immediately before heavy work;
- reserve one host-wide slot and send a terminal handoff;
- classify PASS, PENDING, and NOT_RUN truthfully;
- use GitHub fallback when local evidence is not qualified;
- keep vendor-specific details, private tool settings, and personal data out of
  public CCP artifacts.

The template is a convenience layer. Repository policy and the common contract
remain authoritative.

## 10. Deterministic documentation checks

The implementation tranche will add tests that verify:

- every matrix row has one corresponding harness page;
- each page has all required headings;
- each page declares upstream knowledge, CCP level, evidence reference, and
  verification date;
- no page uses VERIFIED unless it is L4 and points to sanitized evidence;
- the handoff includes exact-head, ownership, fail-closed, and fallback rules;
- public templates do not include known private path patterns, secret markers,
  raw logs, or private code-intelligence settings;
- external URLs are recorded but not fetched during ordinary test runs.

A pattern scan reduces accidental disclosure risk; it does not prove that a
document contains no sensitive information. Human review remains required.

## 11. Rollout, maintenance, and rollback

1. **Design review:** freeze this contract and evidence model.
2. **Reference tranche:** add the common page, matrix, fourteen harness pages,
   and the activity handoff at L1.
3. **Static contract tests:** validate structure, state transitions, links, and
   public-boundary patterns without network or heavy CCP work.
4. **Native evidence:** qualify one harness at a time through L2, L3, then L4.
   Begin with Codex and Claude Code only when their real environments are
   available.
5. **Maintenance:** update a row only after a dated upstream review or new local
   evidence. Keep previous evidence references; do not silently overwrite them.

Rollback is documentation-only. Mark a stale row UNKNOWN or L0, retain the
historical evidence reference, and leave CCP runtime, receipt policy, and
GitHub fallback unchanged.

## 12. Definition of done

### Documentation tranche

- The common contract is readable without prior harness knowledge.
- Fourteen harness pages and matrix rows exist with truthful L1 status.
- The handoff is copy/paste-ready and vendor-neutral.
- Static documentation tests pass without a heavy CCP or OrbStack run.
- No page claims L2, L3, L4, or VERIFIED without exact evidence.

### Native-evidence tranche

- A harness reaches L2, L3, or L4 only with an exact dated, sanitized record.
- An L4 record identifies the exact source SHA, CCP version, trusted policy,
  complete outer result, receipt verification, and rollback note.
- A failure leaves the row at its prior level and documents the limitation
  without weakening CCP safety.

## 13. Upstream sources

- Superpowers current harness list and installation references:
  https://github.com/obra/superpowers
- Superpowers harness-porting invariants, delivery shapes, and acceptance rules:
  https://github.com/obra/superpowers/blob/main/docs/porting-to-a-new-harness.md
- Superpowers Codex reference:
  https://github.com/obra/superpowers/blob/main/docs/README.codex.md
- Superpowers Claude Code plugin reference:
  https://github.com/obra/superpowers/tree/main/.claude-plugin

These sources are reviewed references, not CCP dependencies. No upstream code is
copied into this repository.
