#!/usr/bin/env bash
# Gives the agent a FUNCTION-CALL key: it may only call `pay` on the leash
# contract, and can spend at most 0.25 NEAR on gas. It can't transfer NEAR,
# deploy code, add keys or call any other method.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
PUBKEY=$(node agent/keygen.js)
echo "Agent public key: $PUBKEY"
near account add-key "$AGENT" grant-function-call-access \
  --allowance '0.25 NEAR' --contract-account-id "$LEASH" --function-names pay \
  use-manually-provided-public-key "$PUBKEY" \
  network-config testnet sign-with-legacy-keychain send
