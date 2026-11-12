#!/usr/bin/env bash
# Builds the contract and refreshes the prebuilt copy in contract/res/ that
# bootstrap.sh deploys (so people can try Agent Leash without installing Rust).
set -euo pipefail
cd "$(dirname "$0")/../contract"
cargo near build non-reproducible-wasm
mkdir -p res
cp target/near/agent_leash.wasm res/agent_leash.wasm
sha256sum res/agent_leash.wasm
