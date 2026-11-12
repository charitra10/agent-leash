// Shared setup for the Agent Leash demo scripts.
import { Account, JsonRpcProvider, FailoverRpcProvider, nearToYocto, yoctoToNear, teraToGas } from "near-api-js";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const envFile = new URL("./.env", import.meta.url);
if (existsSync(envFile)) process.loadEnvFile(envFile);

// Account names (match scripts/env.sh; override with environment variables).
export const OWNER = process.env.OWNER ?? "leash-demo-d09e.testnet";
export const LEASH = process.env.LEASH ?? `leash.${OWNER}`;
export const AGENT = process.env.AGENT ?? `agent.${OWNER}`;
export const API = process.env.API ?? `api.${OWNER}`;
export const SCAMMER = process.env.SCAMMER ?? `refund-desk.${OWNER}`;
export const GUARDIAN = process.env.GUARDIAN ?? `guardian.${OWNER}`;
// Free public RPCs rate-limit bursts, so fail over between independent providers.
// (The dashboard prefers fastnear, so the demo prefers NEAR's own RPC.)
export const RPC_URLS = (process.env.RPC_URLS ?? "https://rpc.testnet.near.org,https://test.rpc.fastnear.com")
  .split(",").map((u) => u.trim()).filter(Boolean);

export const provider = new FailoverRpcProvider(RPC_URLS.map((url) => new JsonRpcProvider({ url })));
export { nearToYocto, yoctoToNear };

/** The agent signs with its FUNCTION-CALL key only (agent/.env).
 *  `direct: true` skips failover, for calls we EXPECT the protocol to reject
 *  (failover would retry them on the next RPC and hide the real error). */
export function agentAccount({ direct = false } = {}) {
  const key = process.env.AGENT_PRIVATE_KEY;
  if (!key) throw new Error("AGENT_PRIVATE_KEY missing: run scripts/setup-agent-key.sh first");
  return new Account(AGENT, direct ? new JsonRpcProvider({ url: RPC_URLS[0] }) : provider, key);
}

/** Load an account using the key near-cli saved in ~/.near-credentials/testnet. */
function keychainAccount(accountId, envKey) {
  const key =
    process.env[envKey] ??
    JSON.parse(
      readFileSync(join(homedir(), ".near-credentials", "testnet", `${accountId}.json`), "utf8"),
    ).private_key;
  return new Account(accountId, provider, key);
}

/** The owner signs with their full-access key. */
export const ownerAccount = () => keychainAccount(OWNER, "OWNER_PRIVATE_KEY");

/** The guardian (e.g. a monitoring bot) may only freeze. */
export const guardianAccount = () => keychainAccount(GUARDIAN, "GUARDIAN_PRIVATE_KEY");

/** Freeze needs no deposit, so any key of the owner or a guardian works. */
export const freeze = (account) =>
  account.callFunction({ contractId: LEASH, methodName: "freeze", args: {}, gas: teraToGas("15") });

export const view = (method, args = {}) =>
  provider.callFunction({ contractId: LEASH, method, args });

/** Agent asks the leash to pay someone. Returns the on-chain Attempt record. */
export const pay = (agent, receiver_id, near, memo) =>
  agent.callFunction({
    contractId: LEASH,
    methodName: "pay",
    args: { receiver_id, amount: nearToYocto(near).toString(), memo },
    gas: teraToGas("15"),
  });

/** Owner actions attach exactly 1 yoctoNEAR (forces a full-access key). */
export const ownerCall = (owner, methodName, args = {}) =>
  owner.callFunction({
    contractId: LEASH,
    methodName,
    args,
    gas: teraToGas("30"),
    deposit: 1n,
  });

export const fmt = (yocto) => `${yoctoToNear(BigInt(yocto), 3)} NEAR`;
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
