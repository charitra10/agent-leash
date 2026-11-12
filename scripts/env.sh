# Testnet account names for Agent Leash. Source it: `. scripts/env.sh`
# scripts/bootstrap.sh writes your own deployment to scripts/env.local.sh,
# which overrides the default (the project's public demo deployment).
_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$_here/env.local.sh" ] && . "$_here/env.local.sh"
export NEAR_ENV=testnet
export OWNER=${OWNER:-leash-demo-d09e.testnet}   # human owner (full-access key)
export LEASH=leash.$OWNER                        # the leash contract (holds the funds)
export AGENT=agent.$OWNER                        # the AI agent (function-call key: pay only)
export API=api.$OWNER                            # honest API provider (allowlisted)
export SCAMMER=refund-desk.$OWNER                # "scammer" used in the demo
export GUARDIAN=guardian.$OWNER                  # can freeze, never unfreeze
