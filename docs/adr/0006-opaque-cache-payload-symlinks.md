# ADR 0006: Treat cache-payload symbolic links as opaque objects

- Status: Accepted
- Date: 2026-08-29
- Decision owner: Marco Porcellato
- Design:
  `docs/superpowers/specs/2026-08-29-cache-payload-symlink-design.md`

## Context

CCP's managed cache has a strict link-free control layout, but the `data`
directory contains mutable output produced by containerized project tooling.
Ordinary virtual environments and package-manager caches use symbolic links.
The current producer can promote those payloads and then reject them while
preparing the next generation because its recursive clone and copy helpers
treat every payload link as a control-plane escape.

That contradiction prevents safe persistent-cache reuse and can leave a new
staging directory without cleanup ownership when preparation fails before the
generation owner is constructed.

## Decision

CCP will keep every cache-root, entry, marker, manifest, lock, journal,
generation, payload-root, and ancestry object strictly free of symbolic links.
Only descendants of an already validated plain `data` directory may contain a
symbolic link.

Within that payload plane, CCP treats each link as opaque:

- inspect the link object with no-follow metadata;
- count it as one bounded non-directory object;
- preserve its stored target when copying on supported Unix platforms;
- never stat, open, canonicalize, traverse, or otherwise follow its target on
  the host.

Relative, absolute, broken, recursive, and outside-root target text therefore
does not grant host authority. A project process may later resolve a link only
inside the existing container mount namespace. Cache payloads remain mutable,
unattested performance state.

Windows link-bearing payload reuse remains fail-closed until a separate native
design can preserve reparse semantics without guessing a broken target's file
or directory type.

Prepared-generation cleanup ownership begins immediately after creation of the
owned staging root and before fallible clone or copy work. Cleanup remains
identity-bound, entry-locked, and limited to the exact staging directory.
Whole-generation removal must unlink internal payload links without traversing
their targets; external sentinel fixtures make this a tested security
invariant.

No configuration, cache-key, receipt, policy, generation-manifest, promotion-
journal, or inventory JSON schema changes are made.

## Consequences

Benefits:

- CCP can reuse normal Unix package-manager and environment caches;
- the host no-follow boundary is explicit and testable;
- control-plane link rejection remains unchanged;
- failed preparation no longer creates new unowned staging residue;
- cache and receipt schemas remain compatible.

Costs:

- traversal must use separate strict-control and opaque-payload policies;
- inventory's `files` count explicitly includes payload links;
- Unix fallback copy requires link-preserving logic and platform-specific
  tests;
- Windows link-bearing cache reuse remains unsupported pending native
  qualification;
- clone, copy, inventory, promotion, recovery, and cleanup tests must share the
  same boundary contract.

## Rejected alternatives

- **Disable persistent reuse:** loses the intended performance and credit
  benefit without fixing the lifecycle defect.
- **Delete or rotate affected caches:** mutates operator state and only hides
  the next recurrence.
- **Follow targets that appear contained:** grants authority to untrusted
  payload text and breaks broken or recursive links.
- **Permit links throughout an entry:** weakens ownership, lock, manifest,
  journal, and promotion invariants.
- **Materialize targets as files or directories:** changes cache semantics and
  may copy host data outside the payload.
- **Infer Windows link kind from its target:** cannot safely handle broken or
  outside-root targets.

## Verification gates

1. Boundary tests reject links in every control-plane and payload-root
   position.
2. Unix tests preserve relative, absolute, broken, recursive, and external-
   target links without target traversal.
3. Inventory remains bounded, deterministic, and schema-compatible.
4. Clone and fallback copy produce equivalent link-bearing payloads.
5. Injected preparation failures remove only their exact owned staging path
   while holding the entry lock, without changing external link targets.
6. Promotion and recovery never resolve payload targets.
7. Existing digest, receipt, policy, manifest, journal, and link-free cache
   fixtures remain compatible.
8. Formatting, warnings-denied build, strict Clippy, all-target tests,
   independent review, and one separately authorized two-generation candidate
   qualification pass before any installed producer replacement.

No cache mutation, installed producer replacement, adopter run, receipt
publication, push, PR, merge, or release is authorized by this ADR.
