# Agent Leash

**Give your AI agent a wallet, not your wallet.**

Agent Leash is a NEAR smart-contract wallet that enforces spending rules on an AI agent: per-payment caps, a rolling spending limit, approved recipients, owner approval for larger payments, a freeze switch, and an on-chain log of every blocked attempt. Even a fully prompt-injected agent can't overspend, because the rules live in the contract, not in the prompt.

Built for **NEAR HACKATHON 2026** (tracks: AI Security & Defense · Agent Economy).

> 🚧 Work in progress. The full README (quick start, demo, architecture) is coming before the submission deadline.

## Status
- ✅ Contract (`contract/`, Rust, near-sdk 5) with 16 passing unit tests: `cd contract && cargo test`
- ✅ Live on testnet: [`leash.leash-demo-d09e.testnet`](https://testnet.nearblocks.io/address/leash.leash-demo-d09e.testnet)
- ⏳ Demo agent (`agent/`), dashboard (`dashboard/`)

## Hackathon disclosure
All code in this repository was written from scratch during the hackathon window, starting 2026-11-12. Libraries used: `near-sdk` (Rust) and `near-api-js` (JavaScript).

## License
MIT
