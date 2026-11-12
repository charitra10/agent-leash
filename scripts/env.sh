# Testnet accounts for the Agent Leash demo. Source this: `. scripts/env.sh`
export NEAR_ENV=testnet
export OWNER=leash-demo-d09e.testnet          # human owner (full-access key)
export LEASH=leash.$OWNER                     # the leash contract (holds the funds)
export AGENT=agent.$OWNER                     # the AI agent
export API=api.$OWNER                         # honest API provider (allowlisted)
export SCAMMER=refund-desk.$OWNER             # "scammer" used in the demo
