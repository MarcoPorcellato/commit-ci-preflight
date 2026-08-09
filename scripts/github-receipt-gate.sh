#!/bin/sh
# Copyright 2026 Marco Porcellato
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

set -eu

if [ "$#" -ne 4 ]; then
  echo "usage: github-receipt-gate.sh <verifier> <receipt> <policy> <expected-commit>" >&2
  exit 2
fi

verifier=$1
receipt=$2
policy=$3
expected_commit=$4
summary_path=${GITHUB_STEP_SUMMARY:-}

case "$expected_commit" in
  *[!0-9a-f]*) valid_commit=false ;;
  *) valid_commit=true ;;
esac
case "${#expected_commit}" in
  40|64) ;;
  *) valid_commit=false ;;
esac
if [ "$valid_commit" != true ]; then
  echo "::error title=Commit CI Preflight::The trusted event supplied an invalid commit identifier."
  exit 2
fi
if [ -z "$summary_path" ]; then
  echo "GITHUB_STEP_SUMMARY is required" >&2
  exit 2
fi
if [ ! -x "$verifier" ] || [ ! -f "$policy" ] || [ -L "$policy" ]; then
  echo "::error title=Commit CI Preflight::The trusted verifier or policy is unavailable."
  exit 2
fi
if [ ! -f "$receipt" ] || [ -L "$receipt" ]; then
  echo "::error title=Commit CI Preflight::The commit-bound receipt is missing or is not a regular file."
  exit 3
fi
receipt_size=$(wc -c < "$receipt")
if [ "$receipt_size" -gt 1048576 ]; then
  echo "::error title=Commit CI Preflight::The commit-bound receipt exceeds the one MiB transport limit."
  exit 3
fi

set +e
if [ -n "${CCP_EVALUATED_AT_UTC:-}" ]; then
  verification_output=$("$verifier" verify \
    --receipt "$receipt" \
    --policy "$policy" \
    --expected-commit "$expected_commit" \
    --evaluated-at-utc "$CCP_EVALUATED_AT_UTC" 2>&1)
else
  verification_output=$("$verifier" verify \
    --receipt "$receipt" \
    --policy "$policy" \
    --expected-commit "$expected_commit" 2>&1)
fi
verification_status=$?
set -e

{
  echo "### Commit CI Preflight"
  echo
  echo "The trusted base-branch verifier evaluated the receipt below."
  echo
  echo "~~~text"
  printf '%s\n' "$verification_output"
  echo "~~~"
} >> "$summary_path"

if [ "$verification_status" -ne 0 ]; then
  echo "::error title=Commit CI Preflight::Receipt integrity or repository policy verification failed; see the job summary."
fi
exit "$verification_status"
