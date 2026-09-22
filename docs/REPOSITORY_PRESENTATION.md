# Repository presentation checklist

This file is the source of truth for GitHub-facing product metadata. It keeps
repository presentation accurate without implying that a local file changed a
GitHub setting.

## Recommended repository metadata

- Description: `Run heavy CI locally. Verify exact-commit receipts on GitHub.`
- Website: leave empty until a maintained project site exists.
- Topics: `ci`, `continuous-integration`, `developer-tools`, `devtools`,
  `github-actions`, `local-ci`, `local-first`, `reproducible-builds`, `rust`,
  `supply-chain`.
- Discussions: enable only when a maintainer is ready to moderate adoption and
  design questions.

Changing description, topics, website, Discussions, visibility, or branch
protection is a repository-owner action. Committing this document does not make
those remote changes.

## Social preview

The editable source is [`assets/social-preview.svg`](assets/social-preview.svg).
It is deliberately limited to four large stages so it remains legible in a
small GitHub card:

1. run reviewed checks locally;
2. bind a minimized receipt to the Git commit;
3. verify repository policy independently;
4. publish status for the exact pull-request head.

Before uploading a preview through **Settings → General → Social preview**:

1. review the SVG source and current README claim boundaries;
2. export it to a 1280 × 640 PNG without external fonts, network resources, or
   embedded metadata;
3. visually inspect the PNG at card size;
4. upload it manually and verify the rendered repository page.

The repository does not treat the source SVG as proof that GitHub is currently
using that image.

The rendered `docs/assets/social-preview.png` is the approved upload candidate.
Its presence in source is never proof of the current live GitHub preview; the
live setting must be checked separately.

## Owner-gated live audit

Before changing any GitHub presentation setting, perform a read-only audit of:

1. description, topics, and website;
2. social-preview state and the rendered repository card;
3. Discussions and private vulnerability-reporting availability;
4. the Community Profile and the presence of contribution, security, support,
   conduct, citation, and funding surfaces.

Compare the observed live values with this document and the current README.
Then obtain one explicit owner authorization listing exactly the settings to
change. Apply only those settings, verify the live result read-only, and record
any unavailable or deferred surface truthfully. A source commit, local SVG, or
documentation checklist never changes a GitHub setting.

## Public claim boundary

Use `proof-carrying CI` and `CI receipts for exact Git commits`. Do not claim
zero remote CI, guaranteed savings, producer identity, execution attestation,
or platform qualification without the corresponding evidence described in
[`PRODUCT_ROADMAP.md`](PRODUCT_ROADMAP.md).
