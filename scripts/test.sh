#!/usr/bin/env bash
# Runs every contract test: 29 unit tests + 5 integration tests on a local NEAR sandbox.
set -euo pipefail
cd "$(dirname "$0")/../contract"
cargo near build non-reproducible-wasm   # the sandbox tests deploy this wasm
cargo test
