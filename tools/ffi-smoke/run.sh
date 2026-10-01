#!/usr/bin/env bash
# Builds orbit-ffi as a static library and runs the C smoke test against the
# committed header. Usage: tools/ffi-smoke/run.sh
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
cargo build -p orbit-ffi --release
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
libs="-lpthread -ldl -lm"
if [[ "$(uname)" == "Darwin" ]]; then libs="-framework Security -framework CoreFoundation -framework SystemConfiguration -framework Network"; fi
# shellcheck disable=SC2086
cc -std=c11 -Wall -Wextra -Werror -I crates/orbit-ffi/include \
  tools/ffi-smoke/smoke.c target/release/liborbit_ffi.a $libs -o "$work/smoke"
"$work/smoke" "$work/data"
