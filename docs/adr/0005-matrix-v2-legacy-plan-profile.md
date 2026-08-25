# ADR 0005: Explicit Matrix V2 legacy plan profile

- Status: Accepted
- Date: 2026-08-25
- Decision owner: Marco Porcellato
- Design:
  `docs/superpowers/specs/2026-08-25-matrix-v2-legacy-plan-profile-design.md`

## Context

Matrix V2 configuration schema `2.0` remained parse-compatible while later CCP
versions expanded normalized plan structures. The canonical hashing algorithm
did not change, but the additional serialized fields changed outer and
per-runtime configuration digests. Repositories whose trusted GitHub base still
verifies the historical plan contract cannot accept a receipt from the current
producer even when the configured checks and runtime semantics are unchanged.

Running the historical producer is not an acceptable migration path. It
predates current admission leases and heartbeats and later runtime, recovery,
source-snapshot, cache, and artifact-free execution safeguards. Temporarily
weakening a repository ruleset would also defeat the receipt-first trust
boundary.

## Decision

CCP will support one explicit Matrix-only compatibility profile,
`matrix-v2-legacy-v1`, on `plan`, `doctor`, `dry-run`, and `run`.

The current producer will:

1. normalize the configuration through its current parser;
2. prove that every current semantic field is exactly representable by the
   historical Matrix V2 contract;
3. derive outer and per-runtime digests from dedicated historical
   serialization types reconstructed from source commit
   `044697dee9a0d678d30a4847d62ddf9b4970505b`;
4. execute the equivalent current plan under the modern coordinator and
   runtime safeguards;
5. disclose the selected profile through the producer version while retaining
   the strict historical Matrix V2 receipt shape.

The profile is Matrix V2-only. Single-runtime policy `1.1` performs exact
producer-tuple matching and trusted-plan reconstruction through the current
normalizer; extending that policy requires a separate policy-version decision.

One typed envelope retains the current execution plan and the complete legacy
outer/per-runtime digest bases. Representability and every derived digest are
rechecked before admission, at runtime conversion, and before receipt sealing.
The profile label is disclosure, not authority.

The profile is never inferred from a receipt, policy, filename, repository, or
expected digest. Production code contains no expected repository digest
constants. Unknown profiles and non-representable semantics fail before
admission or shared-state mutation.

The default current profile and its serialized output, digests, runtime
behavior, and receipts remain unchanged.

## Consequences

Benefits:

- trusted-base migrations can retain exact receipt verification without
  executing an obsolete coordinator;
- compatibility is deterministic, independently reproducible, and reusable;
- policies remain external authority and cannot be selected by evidence;
- later execution and recovery hardening remains active.

Costs:

- CCP carries two Matrix V2 canonical representations during migration;
- all four configuration-consuming commands must remain profile-consistent;
- receipt provenance must be disclosed without extending a historical strict
  schema;
- producer-constrained policy `1.1` cannot use this Matrix-only profile;
- every future normalized plan field needs an explicit representability rule or
  a fail-closed rejection.
- the legacy plan digests deliberately create a separate managed-cache
  namespace; current-profile cache entries are not relabelled or promoted.

The change does not introduce a new configuration, policy, receipt, admission,
or cache schema. Matrix receipts remain an outer schema `2.0` envelope with
inner schema `1.0` receipts; the single-runtime policy `1.1` embedded-plan
contract remains out of scope.

## Rejected alternatives

- **Backport modern execution into the historical producer:** too large and
  retains obsolete runtime behavior.
- **Hard-code trusted digests:** non-general, non-auditable, and equivalent to
  a policy exception.
- **Rewrite or translate a receipt after execution:** breaks receipt identity
  and provenance.
- **Ruleset bypass:** weakens the control being migrated.
- **Automatic profile inference:** lets untrusted evidence or ambient state
  influence the digest contract.

## Verification gates

1. Golden fixtures independently reproduce historical generic and adopter
   digests from canonical values.
2. Default output and digests remain byte-identical.
3. Semantic mutations change the derived digest; non-representable fields fail
   before admission.
4. `plan`, `doctor`, `dry-run`, and `run` share one profile-aware plan builder.
5. Historical strict verification accepts a legacy-profile receipt and still
   rejects altered policy, commit, digest, producer evidence, or receipt bytes.
6. Focused tests, formatting, warnings-denied build, strict Clippy,
   all-target tests, independent review, and one separately authorized
   exact-head CCP qualification pass before merge.

No installation, ruleset migration, receipt publication, adopter run, or
scientific execution is authorized by this ADR.
