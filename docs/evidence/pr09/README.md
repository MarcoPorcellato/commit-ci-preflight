# PR09 native evidence matrix

This directory is populated only with receipts from real executions of the
fixed benchmark contract. Source implementation alone does not produce a
platform `PASS`.

| Evidence | Status before qualification | Required proof |
|---|---|---|
| macOS arm64 + OrbStack probe | `PENDING` | Native Mac receipt with runtime flavor `orbstack` |
| Linux x86_64 | `PENDING` | Receipt from standard `ubuntu-24.04` x64 execution |
| Windows x86_64 | `PENDING` | Receipt from standard `windows-2025` x64 execution |
| GitHub-hosted comparison metadata | `PENDING` | Exact run ID, URL, head SHA, runner labels, and conclusions |

Receipts will name the source commit they executed. A later documentation
commit may contain those immutable bytes without pretending they describe the
later commit.

The initial harness pull request does not claim native qualification. GitHub
must first contain the workflow on its default branch; a separately reviewed
evidence pull request records the first manually dispatched run.
