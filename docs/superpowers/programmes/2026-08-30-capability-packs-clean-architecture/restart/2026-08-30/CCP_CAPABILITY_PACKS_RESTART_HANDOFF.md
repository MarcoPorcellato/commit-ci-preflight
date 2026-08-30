# Capability Packs restart handoff — 2026-08-30

This handoff preserves the local Clean Architecture and Capability Packs
programme before a macOS restart. It is a checkpoint, not a qualification
receipt and not authorization to publish or execute heavy work.

## Safe resume point

- Persistent repository:
  `/Users/marco1/Documents/CODICE con VS CODE/commit-ci-preflight`
- Disposable delivery worktree at checkpoint:
  `/private/tmp/ccp-capability-packs-clean-architecture-delivery-v1`
- Durable local branch:
  `codex/capability-packs-clean-architecture-delivery-v1`
- Pre-handoff programme HEAD:
  `1da293a7a1f0e78440f70afe7e2356c56a608535`
- Pre-handoff tree:
  `54368e0190214f00304f3040004842af6931b9c6`
- Base and last fetched `origin/main`:
  `5fed7c443504969e62980141048f9279f9fa1dfe`
- Remote programme branch: absent in the last local ref inventory; not fetched
  or reverified during this restart checkpoint.
- Pull request: none created for this programme.
- Working tree before adding this handoff: clean.

The final restart-checkpoint commit and bundle SHA-256 are recorded in the
persistent external `RECOVERY_MANIFEST.md` next to the bundle. The branch must
contain the pre-handoff programme HEAD above as an ancestor.

## Canonical tracked sources

Read these completely before deciding or implementing anything:

1. `/Users/marco1/.codex/AGENTS.md`
2. `/Users/marco1/.codex/CCP_USAGE.md`
3. `docs/superpowers/specs/2026-08-30-capability-packs-clean-architecture-design.md`
4. `docs/superpowers/plans/2026-08-30-m2-capability-pack-contract.md`
5. `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`
6. `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/goal.txt`
7. this handoff.

## Completed and terminally verified

### M0 compatibility baseline

- M0 is locally closed and independently approved.
- The hash-manifested compatibility corpus and downstream public-facade
  compile fixture are tracked.
- Terminal evidence recorded in the programme progress file includes format,
  manifest, compatibility, diff, and privacy checks.

### M1 private run application seam

- M1 closure commit:
  `b6318ea9837b76a6594258b0dc9f5f66967bd688`.
- Existing public wrappers route through the private dependency seam without
  changing the protected public facade.
- Recorded terminal local evidence: strict Clippy PASS; six compatibility
  contract targets PASS; full host suite 467 passed and 5 ignored across 27
  suites; specification and quality reviews approved with no findings.
- Hosted CI, push, and PR remain unproven.

### M2 plan

- Detailed TDD plan commit:
  `a1c922d4f47fd06e74a336b89a19aecc703908bd`.
- Luna's first review found implicit type/helper/error contracts. They were
  corrected; final bounded review verdict was READY with no Critical or
  Important findings.
- M2 is deliberately library-first: strict bounded TOML, inert validation and
  inspection, explicit single-profile expansion into existing schema-`1.3`
  plan primitives, and separate pack/plan digests.
- M2 must not change current CLI, receipt, policy, configuration, matrix, or
  verification bytes.

## Saved but not yet qualified

- All M2 production implementation is NOT_STARTED and NOT_QUALIFIED.
- The `rust-deep` and `secure-repository` packs are planned only.
- No official pack CLI, tool/image qualification, Docker execution, receipt
  extension, stable installation, tag, package, or release exists from this
  programme.
- The programme branch has not been pushed and has no hosted exact-head result.

## Ignored local evidence preserved separately

The following ignored SDD directories were approximately 272 KiB total and
were copied into the persistent restart archive because the disposable
worktree may disappear:

- `.superpowers/sdd/2026-08-30-m0-compatibility-baseline/`
- `.superpowers/sdd/2026-08-30-m1-run-application-seam/`

They contain briefs, implementer reports, review packages, and ledgers. They
are supporting process evidence. The tracked plans, code, tests, manifests,
and programme progress remain authoritative.

The generated ignored directory
`tests/fixtures/public-api-compat/target/` was approximately 451 MiB and was
not archived. It is rebuildable Cargo output, excluded from evidence, and not
needed to resume.

## Operational state before restart

- Installed executable:
  `/Users/marco1/.cargo/bin/commit-ci-preflight`
- Installed SHA-256:
  `c8021e2322e172686c0a0c07d2b0260eafb5812d085d2306dbbde3fe4e964bd4`
- Installed version: `commit-ci-preflight 0.1.0`
- Admission: `active=false`, queue `0`, slot free.
- Resource policy: `macos-v4`, decision `deny`; available memory 42%,
  reclaimable uncompressed memory 7,969,243,136 bytes, swap used
  10,252,252,610 bytes. The deny is expected to be re-sampled after reboot.
- Recovery: all listed journals were terminal except the preserved historical
  `4911b8ac9cc284b5bbd0c9160747e8c1cebc97c572ef20775e98480092c87665`,
  which remains `operator_required`. Do not modify or reinterpret it.
- Docker context: `orbstack`; no workload was started for this checkpoint.
- Spark restart review: NOT_RUN because the GPT-5.3 Codex Spark usage limit was
  exhausted.
- Luna Git audit: branch and committed work recoverable; it identified the
  ignored SDD ledgers as the missing durable material, which this checkpoint
  archives separately.

## Exact post-restart audit

1. Read every canonical source listed above and the external
   `RECOVERY_MANIFEST.md`.
2. Verify SHA-256 for the Git bundle, SDD archive, copied handoff, copied plan,
   copied specification, copied progress file, and copied prompt.
3. Run `git bundle verify` on the persistent bundle.
4. Inspect the persistent repository with read-only Git commands: local branch
   ref, exact HEAD/tree, `git status`, worktree registrations, remotes, and
   last fetched `origin/main`. Do not prune, repair, delete, reset, clean,
   stash, or overwrite anything.
5. If the `/private/tmp` worktree still exists, use it only if it is clean and
   exactly at the recovery-manifest HEAD. If it is missing or mismatched, do
   not mutate stale worktree registration automatically. Propose either a new
   local clone from the verified bundle or a separately authorized worktree
   repair.
6. Reverify absolute CCP path, complete executable SHA-256, and version.
7. Read-only check `admission status --json`, `resource status --json`,
   `recover status --json`, Docker context, running containers, and relevant
   processes. Preserve journal `4911b8ac...` exactly.
8. Compare live GitHub `origin/main`, any programme branch, PR, and hosted CI
   only after a separately authorized fetch/API read.
9. Return Facts / Unknowns / proposed action / GO or NO-GO before mutation.

## Next programme gate

The next intended external gate remains:

1. read-only fetch of `origin`;
2. proceed only if the reviewed base is unchanged or explicitly reconciled;
3. non-force push of the exact programme branch;
4. open a draft PR;
5. require terminal hosted `Rust CI` on the exact head;
6. only then start M2 Task 1 locally through Subagent-Driven Development.

After restart, all exact hashes must be refreshed before this authorization is
requested or used.

## Boundaries that survive the restart

- The restart handoff is not permission to implement M2 or mutate GitHub.
- Do not run CCP heavy work for this public repository absent a separately
  authorized non-economic exception.
- Do not run Docker workloads, build, full tests, push, create/mutate a PR,
  merge, install, tag, release, publish, prune worktrees, or mutate recovery
  state during the initial audit.
- Preserve the divergent primary checkout and all user work.
- Preserve existing CLI/receipt/policy/plan/schema/public-facade compatibility.
- Live exact-path, exact-hash, Git, CCP, Docker, and GitHub evidence outranks
  this checkpoint when they differ.

