# Agent Leash: Build Plan (NEAR Hackathon 2026)

**One-liner:** *Give your AI agent a wallet, not your wallet.* Agent Leash is a NEAR smart-contract wallet that enforces spending rules on an AI agent: caps, approved recipients, owner approval for big payments, a freeze switch and a tamper-proof log of every blocked attempt.

**Tracks:** AI Security & Defense (primary) · Agent Economy (secondary). **Network:** testnet.
**Deadline:** 14 Nov 2026, 23:59 UTC (15 Nov, 5:29 AM IST). **Target: submitted by 14 Nov, 11:00 PM IST.**
**Model:** Opus only (the user's decision, 2026-11-12).

## Why it matters (story for judges)
Agents are starting to hold money: they pay for APIs, compute and data. Today that usually means pasting a private key into an agent and hoping a prompt injection never tells it to "send everything to this address". Agent Leash moves the trust out of the prompt and into the chain. The agent's key **can only call `pay` on the leash contract**: a NEAR function-call access key with a small gas allowance. Every rule is enforced on-chain, so even a fully hijacked agent can't overspend.

## How it works
```
 owner (human, full-access key)            agent (function-call key: may only call leash.pay)
        │ set_policy / allow / freeze / approve        │ pay(receiver, amount, memo)
        ▼                                              ▼
 ┌──────────────────────── leash contract (holds the funds) ────────────────────────┐
 │ checks in order: frozen → blocklist → allowlist → per-tx cap → window cap        │
 │                  → owner-approval threshold → balance                            │
 │ result: SENT (transfer) │ PENDING (waits for owner) │ BLOCKED (logged with reason)│
 │ every result emits a NEP-297 event (EVENT_JSON) and is kept in on-chain history  │
 └──────────────────────────────────────────────────────────────────────────────────┘
```
- Blocked attempts **don't panic**. They are recorded on-chain with a reason, so the owner sees what the agent *tried* to do.
- Owner methods require **1 yoctoNEAR**, which forces a full-access-key signature. A leaked dapp login key can't loosen the policy.

## Repo layout
| Path | What |
|---|---|
| `contract/` | Rust smart contract (near-sdk) + unit tests |
| `agent/` | Node.js demo agent and the scripted attack scenario (optional LLM mode) |
| `dashboard/` | Static web page that reads the live testnet contract over RPC (hostable on GitHub Pages) |
| `scripts/` | Deploy and setup scripts for testnet |

## Done criteria (each one gets a test or a scripted demo step)
1. Agent pays an allowlisted API provider within limits → **SENT**, balance moves.
2. Prompt-injected agent tries to pay an unknown or scam account → **BLOCKED** (`recipient_not_allowlisted` / `blocklisted_recipient`), no funds move, the attempt is visible in history.
3. Agent tries a single large drain → **BLOCKED** (`over_per_tx_limit`).
4. Agent tries to drain via many small payments → **BLOCKED** once the rolling window cap is hit (`over_window_limit`).
5. A medium payment above the auto-approve limit → **PENDING**. The owner approves → SENT (or rejects).
6. Owner hits **freeze** → every payment is BLOCKED (`frozen`) until unfreeze.
7. The agent's key can't send NEAR directly or call owner methods (shown on-chain with near-cli).
8. Dashboard shows balance, limits, spent-in-window and a live feed of sent / pending / blocked attempts.
9. **Tripwire:** after N blocked attempts within a period (demo: 3 per hour), the contract freezes the agent by itself.
10. **Guardian:** a guardian account can freeze (no deposit needed, so a cheap function-call key works) but can never unfreeze or change rules.

## Timeline (IST)
| When | What | Status |
|---|---|---|
| 12 Nov | Plan, contract + unit tests, tool installs, testnet accounts | done: 16 tests pass, live on testnet at `leash.leash-demo-d09e.testnet` |
| 13 Nov | Deploy to testnet, agent scenario script, dashboard, README | |
| 14 Nov (day) | Polish, hosted dashboard (GitHub Pages), record video | |
| 14 Nov, by 11 PM | X post + submission form (the user does this) | |

**Hard stop:** if criteria 1–6 aren't working on testnet by **14 Nov, 2 PM IST**, freeze features and move to the video.

## Stretch (only after the MVP works)
- LLM mode: a Claude-driven agent reads a "poisoned" email and decides to pay, then the leash blocks it. (Needs an API key; the scripted mode stays the default so judges can reproduce it.)
- Per-recipient caps; time-limited allowances; a guardian account that can only freeze.

## Submission checklist
- [ ] Public GitHub repo with README (what, why, quick start that actually works, testnet contract link), MIT LICENSE, disclosure of libraries used
- [ ] Demo video (~2 min), attached to a public X post tagging @NEARProtocol and #NEARHackathon that says it was built at NEAR HACKATHON
- [ ] Hosted dashboard link (kept live until at least 25 Nov)
- [ ] Form on nearhackathon.com: tracks = AI Security & Defense + Agent Economy; **mainnet** wallet address for the prize
