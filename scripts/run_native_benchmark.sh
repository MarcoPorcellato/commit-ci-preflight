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

if [ "$#" -ne 2 ]; then
  echo "usage: run_native_benchmark.sh <40-character-commit> <new-output.json>" >&2
  exit 2
fi

commit=$1
output=$2

if [ "${#commit}" -ne 40 ]; then
  echo "commit must be exactly 40 lowercase hexadecimal characters" >&2
  exit 2
fi

case "$commit" in
  *[!0-9a-f]*)
    echo "commit must be exactly 40 lowercase hexadecimal characters" >&2
    exit 2
    ;;
esac

cargo run --locked --quiet -- benchmark \
  --commit "$commit" \
  --output "$output" \
  --json
