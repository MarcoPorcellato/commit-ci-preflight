# Repository presentation checklist

This file is the source of truth for GitHub-facing product metadata. It keeps
repository presentation accurate without implying that a local file changed a
GitHub setting.

## Recommended repository metadata

- Description: `Proof-carrying CI: run heavy checks locally, verify exact-commit receipts on GitHub.`
- Website: leave empty until a maintained project site exists.
- Topics: `ci`, `continuous-integration`, `devtools`, `github-actions`,
  `local-first`, `rust`, `supply-chain`, `developer-tools`.
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

## Public claim boundary

Use `proof-carrying CI` and `CI receipts for exact Git commits`. Do not claim
zero remote CI, guaranteed savings, producer identity, execution attestation,
or platform qualification without the corresponding evidence described in
[`PRODUCT_ROADMAP.md`](PRODUCT_ROADMAP.md).
