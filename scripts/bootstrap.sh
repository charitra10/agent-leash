#!/usr/bin/env bash
# One-command setup of your own Agent Leash on NEAR testnet (free, ~2 minutes).
#
# Creates fresh testnet accounts (owner via the free faucet, plus leash, agent,
# api, refund-desk and guardian sub-accounts), deploys the contract, gives the
# agent a function-call key that can ONLY call `pay`, and writes the config the
# demo, MCP server and dashboard need.
#
#   ./scripts/bootstrap.sh            use the prebuilt contract (contract/res/)
#   BUILD=1 ./scripts/bootstrap.sh    rebuild the contract from source first
#   FORCE=1 ./scripts/bootstrap.sh    replace an existing setup (agent/.env)
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
LOG="$ROOT/bootstrap.log"
: > "$LOG"

bold() { printf "\033[1m%s\033[0m\n" "$*"; }
step() { printf "\n\033[1;36m▸ %s\033[0m\n" "$*"; }
run() { "$@" >>"$LOG" 2>&1 || { printf "\033[31mFailed:\033[0m %s\n--- last lines of bootstrap.log ---\n" "$*"; tail -25 "$LOG"; exit 1; }; }
need() { command -v "$1" >/dev/null || { printf "\033[31mMissing %s.\033[0m %s\n" "$1" "$2"; exit 1; }; }

# ---------- prerequisites ----------
need near "Install near-cli-rs: https://github.com/near/near-cli-rs#installation"
need node "Install Node.js 20.12 or newer: https://nodejs.org"
need npm  "It comes with Node.js: https://nodejs.org"
node -e 'const [a,b]=process.versions.node.split(".").map(Number); if (a<20||(a===20&&b<12)) { console.error("Node.js 20.12+ is required, found " + process.versions.node); process.exit(1) }'
if [ -f agent/.env ] && [ "${FORCE:-0}" != 1 ]; then
  echo "agent/.env already exists (you already have a setup). Re-run with FORCE=1 to create a new one."
  exit 1
fi

if [ ! -d agent/node_modules ]; then
  step "Installing Node dependencies"
  run npm --prefix agent ci
fi

WASM="contract/res/agent_leash.wasm"
if [ "${BUILD:-0}" = 1 ]; then
  step "Building the contract from source"
  need cargo "Install Rust: https://rustup.rs"
  run ./scripts/build.sh
fi
[ -f "$WASM" ] || { echo "Contract file $WASM not found. Run BUILD=1 ./scripts/bootstrap.sh"; exit 1; }

# ---------- accounts ----------
SUF=$(node -e 'console.log(require("crypto").randomBytes(3).toString("hex"))')
OWNER="leash-$SUF.testnet"
LEASH="leash.$OWNER"; AGENT="agent.$OWNER"; API="api.$OWNER"; SCAMMER="refund-desk.$OWNER"; GUARDIAN="guardian.$OWNER"

step "Creating owner account $OWNER (free testnet faucet, 10 NEAR)"
run near account create-account sponsor-by-faucet-service "$OWNER" autogenerate-new-keypair \
  save-to-legacy-keychain network-config testnet create

sub() {
  step "Creating $1.$OWNER ($2 NEAR)"
  run near account create-account fund-myself "$1.$OWNER" "$2 NEAR" autogenerate-new-keypair \
    save-to-legacy-keychain sign-as "$OWNER" network-config testnet sign-with-legacy-keychain send
}
sub leash 5          # holds the agent's funds (part pays for contract storage)
sub agent 0.5        # pays its own gas
sub api 0.1          # honest API provider (allowlisted)
sub refund-desk 0.1  # the "scammer" from the poisoned email
sub guardian 0.1     # can freeze, never unfreeze

# ---------- contract ----------
step "Deploying the leash contract to $LEASH"
INIT=$(node -e '
const [owner, agent, api, guardian] = process.argv.slice(1);
const y = (near) => (BigInt(Math.round(near * 1000)) * 10n ** 21n).toString();
console.log(JSON.stringify({
  owner_id: owner, agent_id: agent,
  policy: { max_per_tx: y(0.5), auto_approve_limit: y(0.1), window_limit: y(0.5), window_sec: 86400, allowlist_only: true },
  allowlist: [api], guardians: [guardian], tripwire: { max_blocked: 3, period_sec: 3600 },
}));' "$OWNER" "$AGENT" "$API" "$GUARDIAN")
run near contract deploy "$LEASH" use-file "$WASM" with-init-call new json-args "$INIT" \
  prepaid-gas '30 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-legacy-keychain send

# ---------- agent key ----------
step "Giving the agent a key that can ONLY call pay() on the leash"
KEYS=$(cd agent && node -e 'import("near-api-js").then(({ KeyPair }) => { const k = KeyPair.fromRandom("ed25519"); console.log(k.toString() + " " + k.getPublicKey().toString()); })')
AGENT_SK=${KEYS%% *}; AGENT_PK=${KEYS##* }
umask 077
cat > agent/.env <<EOF
# Created by scripts/bootstrap.sh. Keep secret, never commit (git-ignored).
OWNER=$OWNER
AGENT_PRIVATE_KEY=$AGENT_SK
EOF
umask 022
run near account add-key "$AGENT" grant-function-call-access \
  --allowance '0.25 NEAR' --contract-account-id "$LEASH" --function-names pay \
  use-manually-provided-public-key "$AGENT_PK" network-config testnet sign-with-legacy-keychain send

cat > scripts/env.local.sh <<EOF
# Created by scripts/bootstrap.sh: your own deployment (git-ignored).
export OWNER=$OWNER
EOF

# ---------- done ----------
DASH="https://charitra10.github.io/agent-leash/?contract=$LEASH"
echo
bold "✅ Your Agent Leash is live on testnet"
echo "   owner     $OWNER   (full-access key in ~/.near-credentials/testnet/)"
echo "   leash     $LEASH"
echo "   agent     $AGENT   (function-call key in agent/.env: can only call pay)"
echo "   guardian  $GUARDIAN"
echo
bold "Next:"
echo "   1. Open the live dashboard:  $DASH"
echo "   2. Run the attack demo:      cd agent && npm run demo"
echo "   3. Give an AI agent the wallet (MCP), see README: 'Use it from an AI agent'"
