# Open-source conversion v2 design

## Status and decision

This design defines a trust-first conversion programme for Commit CI Preflight
(CCP) after the public `v0.1.0-rc.2` prerelease. It is a documentation and
community-surface programme, not a change to receipt semantics, admission,
runtime enforcement, pricing, identity, or security boundaries.

The intended reader is a maintainer or developer who has deterministic,
container-friendly CI work and wants a defensible answer to three questions:

1. Does CCP solve my actual cost or feedback-time problem?
2. Can I try it safely without weakening GitHub controls?
3. What evidence and operating responsibility remain mine?

The programme deliberately prioritizes accurate conversion over broad appeal.
CCP must attract economically eligible private-repository users without
claiming monetary savings for ordinary public GitHub-hosted runners.

## Evidence informing the design

The merged RC.2 source is `e4b05c13bef4ed2e458f7e09551618304039b21b`.
The release is public, but several local documentation surfaces still describe
it as a planned or unpublished candidate. That inconsistency is the first
correction because it undermines the release's otherwise explicit evidence
model.

Public comparable projects consistently use a memorable opening, two or three
concrete jobs, a copyable first path, and visible support/trust boundaries.
Their breadth must not be copied as a CCP claim: CCP is neither GitHub Actions
parity, a pipeline SDK, nor a universal cost or performance tool.

Official GitHub guidance supports a clear README, accurate About metadata and
topics, contributor guidance, issue and pull-request templates, an explicit
security policy, releases, and Community Profile completion. Existing CCP
issue forms and pull-request template already serve their purpose and are not
duplicated by this programme.

## Message architecture

The public path has four levels, each answering one progressively deeper
question.

| Level | Reader question | Primary surface | Required message |
|---|---|---|---|
| Recognition | Is this for me? | README opening and repository About | Run eligible heavy CI locally; GitHub verifies exact-head evidence. |
| Qualification | Is it economically and technically appropriate? | README fit table and economic guide | Private billed workloads may qualify; public standard hosted CI does not create billed-minute savings. |
| Trial | Can I verify it before trusting it? | Installation and clean-room tutorial | Verify a named release asset, inspect before execution, then run a disposable example. |
| Adoption | What changes and controls remain? | Adoption guide, security policy, support matrix | Retain remote controls that add trust; adopt only a reviewed explicit plan and policy. |

The opening must state the outcome before mechanism. Receipt, container,
policy, and admission vocabulary follows after the reader knows why it exists.

## README redesign

The README remains the canonical conversion surface. Its first screen will be
restructured around the following ordered blocks.

1. **Outcome-led opening.** Keep the current concise headline, followed by a
   one-paragraph explanation of the user problem and the local/remote split.
2. **Who it is for.** A compact eligibility table with `Good fit`, `Keep
   hosted`, and `Not a fit` outcomes. Economic eligibility and security
   boundaries remain explicit.
3. **Three entry paths.** `Evaluate cost`, `Try safely`, and `Adopt for a
   repository`, each linking to exactly one next document.
4. **How proof works.** Preserve the existing two-row flow diagram, then state
   what a receipt proves and what it does not prove in plain language.
5. **Release-first quickstart.** Prefer release asset checksum verification for
   users evaluating RC.2. Retain source build instructions as an alternative,
   not the default public path.
6. **Evidence and limits.** Give one measured case-study link, the distinction
   between billed savings and local cost, and a short no-go list. Do not add
   synthetic testimonials, stars, download claims, comparative superlatives,
   or universal time claims.
7. **Contribute and get help.** Link to contribution, security, support, and
   issue/discussion routing only after those surfaces exist and are accurate.

The README must state that `v0.1.0-rc.2` is published, unsigned, macOS arm64,
and prerelease. It must never say that a stable package channel, Linux or
Windows runtime qualification, signing, or net financial savings exists.

## Documentation information architecture

The programme preserves existing detailed documents and adds navigation rather
than duplicating contracts.

| Reader need | Canonical document | README label |
|---|---|---|
| Decide whether money can be saved | `docs/ECONOMIC_QUALIFICATION.md` | Evaluate eligibility and savings |
| Compare feedback time honestly | `docs/TIME_TO_FEEDBACK_EVALUATION.md` | Measure time to feedback |
| Verify and try RC.2 | `docs/INSTALLATION.md` and `docs/TUTORIAL.md` | Try safely |
| Adopt in another repository | `docs/ADOPTION_GUIDE.md` | Adopt CCP |
| Understand operational constraints | `docs/BETA_SUPPORT.md` and `docs/THREAT_MODEL.md` | Support and limits |
| Contribute safely | new `CONTRIBUTING.md` | Contribute |

The installation, beta-support, and README status lines will be corrected as
one atomic documentation change. They must link to the actual RC.2 release and
its checksum instructions.

## Trust and proof presentation

Case studies remain useful only when their inputs and limits are visible.
Every public savings or time statement must name its scope and keep these four
categories separate:

1. avoided billed GitHub Actions charges;
2. preserved included quota;
3. avoided remote compute;
4. local electricity, hardware, maintenance, and operator cost.

Time-to-feedback is a separate measured outcome. A faster local run does not
prove a cheaper workflow, and a smaller hosted receipt gate may preserve a
required remote control without eliminating all hosted usage.

The programme may add concise callouts and navigation to existing evidence. It
does not create new counterfactual numbers, rename estimates as observations,
or publish machine-specific details.

## Community and repository-health surfaces

### Included in the first implementation tranche

- Create `CONTRIBUTING.md` in the repository root. It will direct contributors
  to scope, local checks, public-claim boundaries, issue forms, pull-request
  evidence, and documentation conventions without duplicating the existing PR
  template.
- Audit current GitHub About description, topics, social preview, and release
  presentation against documented RC.2 facts. Change only inaccurate or absent
  metadata after a separate owner authorization.
- Link the existing issue forms and PR template from the README where they are
  useful to newcomers.

### Deferred pending explicit maintainer choices

- `SECURITY.md`: add only after confirming an active private vulnerability
  reporting route and supported-version policy.
- `CODE_OF_CONDUCT.md`, `SUPPORT.md`, and Discussions: add only after defining
  a monitorable response/routing policy. No response-time promise is implied.
- `CITATION.cff`: add only if the maintainer wants a formal software-citation
  surface and approves correct author/version metadata.
- `.github/FUNDING.yml`: add only for an actual, approved funding route.

This prevents empty governance files from becoming misleading conversion
theater.

## Measurement and maintenance

Success is measured with reviewable repository signals, not vanity claims:

- release-asset checksum verification remains reproducible;
- first-time users can reach one named safe trial path from the README;
- issue forms produce complete, sanitized reports;
- contributor and security routes do not conflict;
- public economic claims remain bounded and source-linked;
- release/status documentation agrees with live GitHub state.

Future conversion instrumentation must be privacy-preserving and opt-in. This
programme adds no telemetry, analytics SDK, tracking pixel, or user-data
collection.

## Delivery slices

### PR A: Published-release consistency and README conversion

Correct RC.2 status language in README, installation, and beta-support
documents. Restructure README's first screen and entry paths; retain all
existing evidence links and limits. Add deterministic documentation tests for
release status, required entry links, and prohibited public-runner savings
claims.

### PR B: Contributor route and navigation

Add `CONTRIBUTING.md`, link existing issue/PR surfaces, and improve document
navigation. Add contract tests for authoritative links and contribution-route
presence. Do not add security or support promises in this slice.

### PR C: Maintainer-approved GitHub health metadata

After a live audit and separate authorization, correct About/topics/social
preview only where needed. Decide independently whether to enable Discussions,
private vulnerability reporting, a code of conduct, support, citation, or
funding. This slice may have no source diff if current settings already match
the approved policy.

## Acceptance criteria

- All public status language agrees with published `v0.1.0-rc.2` facts.
- A first-time reader can find an eligibility decision, safe trial, adoption
  path, measured-evidence method, and limitation statement from README.
- Documentation never calls public standard hosted CI a billable-savings use
  case and never guarantees time or net monetary savings.
- Existing issue forms, PR template, release assets, threat model, and detailed
  operating contracts remain authoritative and linked without duplication.
- Contributor/security/community features are introduced only when their
  operational promises are true and maintainable.
- Documentation tests and complete deterministic validation pass before a
  publication request.

## Explicit non-goals

- No product CLI, receipt, schema, policy, admission, cache, runtime, or
  verifier behavior changes.
- No GitHub setting change, branch protection change, Discussions enablement,
  funding route, security-reporting route, telemetry, or remote mutation in
  this design.
- No claim of GitHub Actions parity, local/hosted equivalence, universal speed,
  universal savings, signed identity, or stable multi-platform support.
