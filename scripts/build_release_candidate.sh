#!/usr/bin/env bash
# Copyright 2026 Marco Porcellato
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

usage() {
  echo "usage: scripts/build_release_candidate.sh /absolute/output/directory" >&2
  exit 64
}

if [[ "$#" -ne 1 ]]; then
  usage
fi

output_dir="$1"
case "$output_dir" in
  /*) ;;
  *)
    echo "release candidate output directory must be absolute" >&2
    exit 64
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  echo "release candidate build requires a clean Git checkout" >&2
  exit 2
fi

cargo run --locked --quiet --example generate_release_metadata -- --check
cargo test --locked --quiet --test release_hardening_contract
cargo build --locked --release --bin commit-ci-preflight

package_id="$(cargo pkgid -p commit-ci-preflight)"
version="${package_id##*@}"
if [[ "$version" == "$package_id" || -z "$version" ]]; then
  echo "could not derive package version from cargo pkgid" >&2
  exit 2
fi

target="$(rustc -vV | awk '/^host: / { print $2 }')"
if [[ -z "$target" ]]; then
  echo "could not derive the Rust host target" >&2
  exit 2
fi

archive_base="commit-ci-preflight-v${version}-${target}"
archive_path="${output_dir}/${archive_base}.tar.gz"
checksum_path="${output_dir}/SHA256SUMS"
if [[ -e "$archive_path" || -e "$checksum_path" ]]; then
  echo "refusing to overwrite an existing release candidate or checksum manifest" >&2
  exit 2
fi

mkdir -p "$output_dir"
stage_root="$(mktemp -d "${TMPDIR:-/tmp}/ccp-release-candidate.XXXXXX")"
stage_dir="${stage_root}/${archive_base}"
cleanup() {
  rm -rf -- "$stage_root"
}
trap cleanup EXIT INT TERM

mkdir -p "$stage_dir/docs"
install -m 0755 target/release/commit-ci-preflight "$stage_dir/commit-ci-preflight"
install -m 0644 LICENSE NOTICE README.md SBOM.spdx.json THIRD_PARTY_NOTICES.md "$stage_dir/"
install -m 0644   docs/INSTALLATION.md   docs/UPGRADE_AND_ROLLBACK.md   docs/THREAT_MODEL.md   docs/BETA_SUPPORT.md   docs/TUTORIAL.md   "$stage_dir/docs/"

COPYFILE_DISABLE=1 tar -czf "$archive_path" -C "$stage_root" "$archive_base"

archive_name="${archive_base}.tar.gz"
if command -v shasum >/dev/null 2>&1; then
  (
    cd "$output_dir"
    shasum -a 256 "$archive_name" > SHA256SUMS
  )
elif command -v sha256sum >/dev/null 2>&1; then
  (
    cd "$output_dir"
    sha256sum "$archive_name" > SHA256SUMS
  )
else
  echo "neither shasum nor sha256sum is available" >&2
  exit 2
fi

echo "release candidate: $archive_path"
echo "checksums: $checksum_path"
echo "no tag, signature, upload, package publication, or GitHub Release was created"
