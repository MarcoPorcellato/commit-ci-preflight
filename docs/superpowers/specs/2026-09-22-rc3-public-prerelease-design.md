# v0.1.0-rc.3 Public Prerelease Design

## Decision

Prepare a narrow `v0.1.0-rc.3` GitHub prerelease from one exact, clean source
commit after `v0.1.0-rc.2`. This is not a stable `v0.1.0` release and does not
relax its Definition of Done.

The candidate must distribute the current public documentation and the explicit
fail-closed admission-reconciliation capability added after RC.2. It must keep
the existing distribution boundary: one unsigned macOS arm64 archive plus a
checksum manifest, no package registry, installer, signing, automatic update,
Linux binary, or Windows binary.

## Intended source and scope

The initial candidate source is `main` at
`493eea49381e16f973dd3749a0b12f6502e25a90`. A later source may be selected
only if it is independently reviewed, clean, and all qualification evidence is
rebound to its complete commit.

RC.3 includes the merged changes after RC.2:

- clearer public onboarding and contributor routing;
- checksum-first installation guidance;
- explicit bounded admission reconciliation, including read-only preview,
  selected-ticket apply, strict ownership checks, and operator boundaries.

It excludes stale PR #64 and draft PR #74. Neither is required for this
prerelease and neither may be represented as included.

## Documentation rule

Source documentation must not hard-code a prerelease label as the currently
published artifact. It must instead direct readers to the GitHub Releases page
and identify a downloaded archive by its adjacent `SHA256SUMS` and in-archive
`RELEASE_MANIFEST.json`. This keeps the checked-in guidance truthful both
before and after a future RC is published.

The RC.3 release notes may name `v0.1.0-rc.3`, its exact source commit, archive
name, checksum, and evidence only after those values have been generated and
independently verified. They must preserve every RC.2 non-claim: unsigned,
macOS arm64 only, prerelease only, no universal time or monetary saving claim,
and no identity attestation.

## Qualification gates

Before publication, the selected candidate must provide:

1. clean selected source and deterministic release metadata;
2. format, Clippy with warnings denied, documentation build, and complete test
   suite passing;
3. a locally built macOS arm64 candidate archive, `SHA256SUMS`, and an
   in-archive manifest whose release label, source commit, target, and archive
   name agree;
4. checksum verification after archive creation;
5. a bounded isolated install and rollback smoke check on macOS arm64;
6. hosted CI for the exact source when a release-preparation PR is opened;
7. independent final verification of the uploaded asset digest after upload.

Local candidate build and smoke checks do not create a tag, GitHub Release,
upload, signature, package, installer, or stable-binary replacement.

## External authorization boundary

The following remain separate maintainer actions and are not authorized by
local preparation: creating tag `v0.1.0-rc.3`, creating or editing the GitHub
Release, uploading `SHA256SUMS` or the archive, publishing the release, or
announcing it.
