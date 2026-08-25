# Matrix V2 legacy plan profile design

Status: proposed for owner review  
Date: 2026-08-25  
Baseline: `2b4b55ce1a4be0a2b610656ae4a56a7641b29f26`  
Historical plan authority: `044697dee9a0d678d30a4847d62ddf9b4970505b`  
Scope: let the current producer execute a semantically representable Matrix V2
configuration under the exact historical digest contract without weakening the
current coordinator, runtime, receipt, or verification boundaries

## Decision

CCP will add one explicit Matrix-only plan profile named
`matrix-v2-legacy-v1`. The profile will reconstruct the historical canonical
plan representation from normalized inputs and use that representation as the
configuration-digest authority while the current producer retains its current
admission coordinator, runtime containment, source snapshot, cache, recovery,
and artifact safeguards.

The current profile remains the default and remains byte-for-byte unchanged.
The legacy profile is opt-in on `plan`, `doctor`, `dry-run`, and `run`; it is
never inferred from a repository policy, receipt, digest, filename, or expected
hash.

The implementation must not contain the three Latent-TRIZ expected digest
values or any lookup table that maps a repository/configuration to a digest.
Golden tests may record expected outputs, but production code must derive every
digest from canonical serialized values.

## Problem and exact evidence

Latent-TRIZ PR #105 is frozen at source
`34b52c42ef08cfe7043dde53f300154cc01d22f9` with trusted base
`188eb65b5e249923baddadeba52659f07fcd1609`.

The current installed producer, source
`3fccc197e5055a2759ee7afe51b91133938ec904`, generated a terminal passing local
Matrix receipt with these digests:

- outer: `25b35b942a6ff9b6237ebed7cefbdbc96b968bbe8954a38b606942f36b8df4b2`;
- Python 3.11:
  `b3d8beef1542566d9d925bfee77d2244995dc74adcd879128ef65e82ed1d354b`;
- Python 3.12:
  `d446c4ca0602c09eee61c796ad2972f58ab0eebe84a39f928fd90aac5bfb535c`.

The trusted GitHub workflow builds historical CCP source
`044697dee9a0d678d30a4847d62ddf9b4970505b` and expects:

- outer: `13f4cb39b7e1a8ed31cae64502cc8e4d80d040230d3fb410a6afc3bad3b76178`;
- Python 3.11:
  `eff5b7d55bb0220890dbfb050bb68a1e0fbba8f9a30a69e2f66085354fcc8562`;
- Python 3.12:
  `7afb3e6dd435d9d5a317e4d9d85e80527431044312bbe299e9a70b6ba9e994c8`.

The hosted verifier accepted receipt integrity and rejected only the three
policy/configuration bindings. An isolated build of the historical producer
reproduced all three trusted digests, proving the mismatch is deterministic.

SHA-256 canonicalization itself did not change. The serialized plan shape did:

- `environment_allow` became a structured `environment` value;
- newly normalized runtime, storage, artifact, and fixed-environment fields
  entered current internal plans, even when their values are default or empty;
- Matrix runtime plan projection therefore emits different canonical bytes.

The old producer cannot safely be used as the execution engine. It predates
the current lease/heartbeat coordinator and later runtime, artifact-free,
source-snapshot, cache, and cleanup hardening.

## Alternatives considered

### A. Backport the current coordinator and runtime fixes onto the historical producer

Rejected. Admission changes span large ownership, lease, quarantine, recovery,
status, and CLI surfaces. The historical producer also predates other execution
fixes. A migration-only fork would be harder to qualify and would retain older
runtime behavior merely to preserve serialization.

### B. Temporarily bypass or weaken the GitHub ruleset

Rejected. A one-time administrative policy bypass would make the trusted-base
transition possible but would remove the exact control the receipt-first design
is intended to preserve. It would also provide no reusable solution for other
repositories with historical trusted digests.

### C. Add a versioned legacy plan projection to the current producer

Selected. This preserves the current execution engine, keeps compatibility
explicit and fail-closed, and makes the historical digest a deterministic
result of a reviewed projection rather than a privileged constant.

## CLI contract

The four configuration-consuming command families gain the same optional
argument:

```console
--matrix-plan-profile current-v2
--matrix-plan-profile matrix-v2-legacy-v1
```

The option is accepted by:

```console
commit-ci-preflight plan
commit-ci-preflight doctor
commit-ci-preflight dry-run
commit-ci-preflight run
```

`current-v2` is the default. Omitting the option must preserve current command
output, plan digests, execution behavior, and receipts exactly.

Supplying `matrix-v2-legacy-v1` with a non-Matrix configuration fails before
runtime, admission, cache, workspace, journal, or receipt mutation.

The profile is a caller selection, not repository authority. `verify` continues
to use only the externally selected repository policy and expected commit. A
receipt cannot request a profile, select a policy, or authorize its own digest.

This tranche supports Matrix V2 policy only. It does not add legacy projection
to single-runtime policy `1.1`, whose trusted-plan loader reconstructs the
current plan directly from trusted configuration. Supporting that policy would
require a separately versioned policy field and verifier design. The CLI
therefore rejects the legacy Matrix profile for every single-runtime config.

## Canonical representation

### Profile type

Introduce a closed `MatrixPlanProfile` enum with two variants:

- `CurrentV2`;
- `LegacyV1` serialized for CLI use as `matrix-v2-legacy-v1`.

Parsing rejects unknown names and profile aliases. The enum is shared by the
four command paths so they cannot silently construct different plans.

### Dual representation

The legacy profile has two related values:

1. a **legacy digest basis** that reproduces the exact historical serialized
   structures;
2. a **current execution plan** used by the modern runtime.

The digest basis uses dedicated private serialization types reconstructed from
historical source `044697dee...`. They contain exactly the historical fields:

- outer Matrix plan: schema version, project, receipt, `environment_allow`,
  caches, and runtime plans;
- per-runtime plan: schema version, project, normalized runtime, receipt,
  `environment_allow`, caches, and checks;
- runtime and check representations omit fields that did not exist in the
  historical plan.

The projection is computed from normalized semantic values, not TOML bytes.
Whitespace, table order, and map-key order therefore retain current canonical
behavior.

The current execution plan remains the source for actual runtime construction.
Before returning a legacy envelope, CCP proves that every current-only semantic
field is representable by the historical basis. Any non-default storage,
fixed-environment value, runtime pull/swap policy, artifact contract, or future
field without an explicit equivalence rule fails with a typed
`LegacyPlanNotRepresentable` error naming the first rejected field.

No field may be silently dropped merely because its serialized value is empty.
The equivalence rule for every omitted field must be explicit and tested.

### Digest verification

The plan builder returns one typed, profile-bearing envelope that owns both the
current execution plan and, for the legacy profile, the complete historical
outer/per-runtime digest bases. Current-profile canonical verification
continues to recompute the digest from the current plan. Legacy verification
recomputes every digest from the legacy bases and re-runs the semantic
representability proof.

The envelope stores no independently writable digest override. Its exposed
outer and per-runtime digests are derived accessors. The builder, runtime
envelope conversion, pre-admission check, per-runtime execution boundary, and
pre-seal receipt path each recheck the same invariant:

```text
current normalized execution plan
    -> exact representability proof
    -> historical per-runtime bases and derived digests
    -> historical outer basis and derived digest
    -> matching inner and outer receipt configuration digests
```

Any mismatch is terminal and cannot publish a receipt. The profile label is
disclosure only; it is never accepted as proof that this invariant holds.

The default JSON output must remain unchanged. Legacy `plan --json` adds a
top-level `matrix_plan_profile: "matrix-v2-legacy-v1"` disclosure outside the
hashed plan value and exposes the normalized legacy digest basis so an
independent implementation can reproduce the digest.

The three expected Latent-TRIZ hashes appear only in fixtures/tests and adopter
documentation, never in the profile implementation.

## Runtime and receipt behavior

The modern producer executes the current plan only after the legacy projection
and both outer/per-runtime digest checks succeed.

Every runtime receipt and the outer Matrix receipt use the derived legacy
configuration digest. The current Matrix V2 receipt schema remains unchanged so
historical strict verifiers can parse it.

To disclose the compatibility path without adding a receipt field that an old
strict Matrix parser would reject, the producer version in all inner and outer
receipts gains the deterministic suffix:

```text
0.1.0+matrix-v2-legacy-v1
```

The default profile continues to emit the ordinary package version. The
profile-specific producer version must be set before sealing inner receipts;
all inner and outer receipts must agree exactly.

This suffix is compatible with the current Matrix V2 policy because that policy
does not contain a producer allowlist. It is not universally compatible with
single-runtime policy `1.1`, which matches producer name/version tuples exactly.
The profile is unavailable to that command path. If a future Matrix policy adds
producer constraints, the trusted policy must explicitly allow this exact
suffix; CCP must not infer prefix or semantic-version compatibility.

The legacy profile does not weaken:

- clean exact-HEAD requirements;
- resource and admission gates;
- lease/heartbeat coordination;
- Docker/runtime probes and containment;
- source snapshot and repository evidence;
- check argv, timeout, runtime image, and platform evidence;
- atomic receipt publication and no-overwrite rules;
- independent verification against an externally supplied policy.

## Error and recovery behavior

Profile parsing, representability, canonicalization, or digest mismatch errors
are pre-execution failures. They must produce:

- no admission ticket or lease;
- no Docker/container call;
- no cache generation or pin;
- no run journal;
- no receipt;
- no source-tree mutation.

After admission begins, current terminal failure, recovery, cleanup, and receipt
rules apply unchanged. The legacy profile does not authorize retries or convert
an inconclusive/failed terminal outcome to PASS.

## Verification strategy

Implementation is test-driven. Each production change follows a witnessed
RED-GREEN cycle.

### Golden compatibility fixtures

Store one generic two-runtime Matrix configuration that is valid at both the
historical and current commits. Generate the historical expected JSON and
digests from an independently built exact historical producer and retain this
fixture provenance:

- source commit: `044697dee9a0d678d30a4847d62ddf9b4970505b`;
- source tree: `5220164edf17831ce0c42dae1c14300ed1045015`;
- isolated binary SHA-256:
  `71d64cdbb1bb509bb459aebd6c53e06d819150de42be4fe3715c35bd73426af7`;
- exact plan command and raw canonical output.

The fixture is not Latent-TRIZ-specific. A separate adopter regression may use
the public Latent-TRIZ configuration and trusted digests as end-to-end evidence.

Tests must prove:

- legacy outer and per-runtime digests equal the historical producer;
- current profile digests remain exactly unchanged;
- reordering TOML tables or keys does not change either profile;
- changing any semantic field changes the applicable digest;
- no expected digest string exists in production source;
- default CLI output is byte-identical to the baseline fixture;
- legacy JSON discloses the profile and reconstructible digest basis.

### Fail-closed profile tests

Add negative fixtures for:

- legacy profile with a single-runtime configuration;
- unknown or conflicting profile values;
- non-default current-only storage behavior;
- fixed environment values;
- runtime pull/swap policy;
- artifact contracts;
- mutation of the legacy digest basis after plan construction;
- mismatch between inner and outer profile/provenance;
- receipt-selected policy or expected digest attempts.

Every pre-execution failure asserts zero calls to admission, runtime,
supervisor, cache, journal, and receipt publication ports.

The positive end-to-end fixture must pass the actual verifier built from
historical source `044697dee...` against an externally selected Matrix V2
policy. Negative mutations of producer evidence, expected commit, outer digest,
per-runtime digest, required check, runtime binding, and receipt bytes must fail
through that same verifier. Parse success alone is not acceptance evidence.

### Command parity

For the same configuration/profile, `plan`, `doctor`, `dry-run`, and `run`
must observe the same outer and per-runtime digests. Doctor and dry-run remain
non-executing with respect to project checks; plan remains runtime-free.

Focused suites cover matrix, plan CLI, runtime CLI, receipts, verification, and
admission non-acquisition. The final static qualification runs formatting,
warnings-denied checks, strict Clippy, documentation checks, and all locked
targets without Docker or a CCP heavy run.

## Documentation

Update:

- `docs/CONFIGURATION.md` with the profile and representability boundary;
- `docs/MULTI_RUNTIME_RECEIPTS.md` with dual-plan semantics and producer
  disclosure;
- `docs/RECEIPT_SPEC.md` with the profile-specific producer version;
- `docs/LOCAL_RUN.md` with the four-command parity rule;
- `docs/GITHUB_GATE.md` and `docs/ADOPTION_GUIDE.md` with the bootstrap
  migration sequence;
- `docs/INVARIANT_EVIDENCE_MATRIX.md` and
  `docs/TESTING_AND_FAULT_INJECTION.md` with proof obligations.

The public term is `legacy plan profile`. Documentation must not describe the
profile as a digest override, policy bypass, or receipt rewrite.

## Delivery and migration sequence

1. Implement and statically qualify the profile from this exact current
   baseline or a newly verified `origin/main` descendant.
2. Obtain independent architecture/security review.
3. Build one isolated hash-bound candidate and prove both current and legacy
   plan outputs without installing it.
4. Separately authorize and perform one CCP exact-head qualification of the CCP
   candidate using the ordinary current profile.
5. Publish and merge the CCP change only after its exact-head gates pass.
6. Use the qualified candidate by absolute path for a fresh Latent-TRIZ
   `plan`, `doctor`, and `dry-run` under `matrix-v2-legacy-v1`.
7. Stop for a new authorization binding the Latent-TRIZ source HEAD, candidate
   SHA-256, profile, generation, maximum run count, and stop boundary.
8. After one terminal run, independently verify the receipt against the
   trusted-base policy and expected commit.
9. Only then request separate authorization to publish the replacement evidence
   branch, update PR #105 if its exact head/base still match, and merge after
   all hosted gates are green.
10. Adopt the proposed Latent-TRIZ static-analysis stack only in a later,
    separately planned Matrix migration so this bootstrap fix remains isolated.

## Non-goals

- No hard-coded repository, policy, or digest exception.
- No automatic profile inference.
- No admission-root migration or legacy coordinator execution.
- No ruleset bypass or reduction of required checks.
- No receipt editing, translation, or re-signing after execution.
- No support for current-only semantics that lack an exact historical
  representation.
- No model, tokenizer, sealed-target, or scientific execution.
- No change to Latent-TRIZ scientific claims or frozen protocols.

## Completion criteria

The compatibility feature is complete only when all of the following are
proven:

- the default profile is byte- and digest-stable;
- the legacy projection independently reproduces the historical generic and
  Latent-TRIZ fixtures without production digest constants;
- all four command families share one profile-aware planning path;
- incompatible semantics fail before shared-state mutation;
- receipts disclose the legacy profile through producer version and remain
  accepted by the historical strict Matrix V2 verifier and its exact external
  policy; no compatibility claim is made for producer-constrained v1.1 policy;
- current admission, runtime, cache, recovery, and receipt invariants remain
  qualified;
- the CCP change is independently reviewed and exact-head qualified;
- the Latent-TRIZ replacement run, publication, and PR merge each occur only
  under their own exact authorization and terminal gates.
