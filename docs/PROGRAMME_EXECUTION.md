# Commit CI Preflight programme execution ledger

## Document control

| Field | Value |
| --- | --- |
| Status | Active delivery ledger; it records evidence and dependencies, not release approval |
| Local source anchor | `origin/main` at `5f2ef665be4dc47fd354befcba53251a4e51744f`, inspected on 2026-08-20 |
| Remote-state caveat | GitHub DNS was unavailable during this inspection. Re-verify remote HEAD, open PRs, and checks before every publication or merge. |
| Authority | Marco Porcellato approves implementation under the repository's existing authority model. Signing, registry publication, paid services, secrets, platform claims, and stable release still require their separate gates. |
| Normative companions | [Product roadmap](PRODUCT_ROADMAP.md), [reliability hardening plan](RELIABILITY_HARDENING_PLAN.md), [implementation history](IMPLEMENTATION_PLAN.md) |

## Purpose

This ledger makes the programme executable without treating a historical plan,
a merged source change, a local test, or an incomplete receipt as interchangeable
evidence. It resolves work in dependency order and keeps three states separate:

- **implemented:** source is present on the stated anchor;
- **qualified:** the exact candidate has the required terminal evidence;
- **released:** an owner-authorized external publication has completed.

Only a terminal exact-head receipt and its required independent verification
qualify a local preflight. A hosted fallback remains required whenever that
evidence is missing, stale, incomplete, or unverifiable.

## Protected local work

The primary checkout was deliberately not used for this programme tranche. At
inspection it was divergent and contained an unresolved, user-owned Receipt v2
index conflict plus related staged source and test changes. That work must be
preserved and reconciled only in its own reviewable branch. This ledger neither
adopts nor validates it.

## Programme map

| Order | Workstream | Current evidence | Next source slice | Qualification or authority gate |
| --- | --- | --- | --- | --- |
| P0 | Baseline reconciliation | PR 57 design is merged at the local anchor; historical plans contain older anchors and candidate-only language. | Reconcile dated anchors and closed/superseded milestones when a clean current branch is available. | Live remote, open-PR, and exact-head recheck before publication. |
| P1 | Environment and plan truthfulness | `environment.allow` carries names only; the runtime inherits an allowlisted host value. The repository Rust preflight needs `CARGO_HOME`, `CARGO_TARGET_DIR`, and `RUSTUP_HOME` to agree with mounted managed caches. | T8 environment classes and trusted-plan binding: fixed, runtime-internal, and remote-secret-only values. | T7/T8 golden vectors must prove a changed normative field fails verification. No secret value enters a receipt. |
| P2 | Receipt v2 and immutable source | T2/T3/T4/T5/T6 candidate work is described, but qualification is candidate-specific and must not be inherited. | Integrate the independently preserved Receipt v2 work only after conflict review; complete T7 plan reconstruction. | Fresh exact-head receipt, independent verifier, cleanup evidence, and required native evidence. |
| P3 | Reliability P0/P1 closure | T0-T6 have candidate source progress; T7-T11 retain required work. | Ordered T7, T8, T9, T10, then T11; do not add unrelated runtime features. | Every tranche meets its own deterministic and native exit criteria in the reliability plan. |
| P4 | One-command adoption | Existing docs describe safe manual commands and receipt publication mechanics. | Product PR 5 and PR 6: deterministic `check`, `init`, adoption proposal, GitHub setup, and transactional publication. | Clean-room Rust, Python, and Node proof; source/ref mutation tests in isolated local bare repositories. |
| P5 | Receipt-first remote cost reduction | The A0 remote gate and exact-receipt model exist; cost reduction must remain measured, not promised. | Product PR 2-3 verifier split and slim trusted gate, then PR 7 local-only cost reports. | Immutable verifier identity, bounded remote gate, golden cost fixtures, and current official pricing data. |
| P6 | Multi-harness reference | The vendor-neutral design is merged. All listed harnesses are L1 only. | Add common contract, compatibility matrix, fourteen L1 pages, reusable handoff, and static documentation checks. | L2-L4 only after a dated, sanitized fresh-session record; no global harness configuration is modified by CCP. |
| P7 | Distribution and platforms | No stable package, signing identity, or cross-platform claim is inferred from source. | Product PR 4 then PR 8: distribution design and genuine Linux x86_64 qualification. | Explicit owner authorization for signing/registries; real native receipts for each promoted platform. |
| P8 | Assurance and stable release | A0 is the only present assurance claim. | Product PR 9 A1 design/implementation and PR 10 external evidence. | Separate signing authority, three consented case studies or a reviewed roadmap change, and all stable-release criteria. |

## Near-term execution order

1. **T7/T8 boundary ADR and contract tests.** Specify how a value becomes a
   fixed trusted value, a deterministic runtime-internal value, or a remote-only
   secret. The current Rust-cache export failure is a regression fixture, not a
   reason to silently invent operator inputs.
2. **Receipt v2 integration review.** Rebase or reconcile the protected local
   work in a dedicated branch. Preserve v1 parsing and historic fixtures.
3. **T7 trusted-plan reconstruction.** Make verifier comparison field-complete
   before adding convenience automation that depends on plan semantics.
4. **T8 implementation.** Add declared environment classes, artifact manifests,
   disk/runtime capability contracts, and deterministic tests. The Rust-cache
   default becomes runtime-internal only if its container target is declared in
   the normalized plan and independently verified.
5. **Multi-harness L1 documentation tranche.** This is documentation-only and
   can proceed in parallel once the common contract is linked to the current
   admission ownership rules.
6. **T9-T11 plus productization.** Physical verifier split, evidence
   immutability, resource/history correctness, chaos/native proof, adoption,
   cost reports, distribution, and external evidence remain separately gated.

## Decision and stop rules

- Do not represent a green inner container stage after an outer guard failure
  as a PASS.
- Do not regard an absent process in one shell as proof that the host-wide
  admission slot is inactive.
- Do not retry a denied or unknown resource admission; diagnose the control
  plane first and preserve receipts and owned state.
- Do not publish a verifier, create signing material, publish packages, or
  claim a native platform without the explicit authority and direct evidence
  required by the product roadmap.
- Do not place vendor-specific agent settings, home paths, raw logs, secrets,
  private code, or code-intelligence configuration in public documentation.

## Checkpoint record

Before beginning any heavy qualification, record the exact commit, clean
worktree state, resource decision, admission ownership state, runtime
responsiveness, planned command, and file allowlist. At terminal state record
the outer outcome, exit code, cleanup status, independent receipt result, and
post-run admission/runtime state. An incomplete record is **PENDING**, never
PASS.
