// Shared setup for the Agent Leash demo scripts.
import { Account, JsonRpcProvider, nearToYocto, yoctoToNear, teraToGas } from "near-api-js";
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
export const RPC_URL = process.env.RPC_URL ?? "https://test.rpc.fastnear.com";

export const provider = new JsonRpcProvider({ url: RPC_URL });
export { nearToYocto, yoctoToNear };

/** The agent signs with its FUNCTION-CALL key only (agent/.env). */
export function agentAccount() {
  const key = process.env.AGENT_PRIVATE_KEY;
  if (!key) throw new Error("AGENT_PRIVATE_KEY missing: run scripts/setup-agent-key.sh first");
  return new Account(AGENT, provider, key);
}

/** The owner signs with the full-access key that near-cli saved. */
export function ownerAccount() {
  const key =
    process.env.OWNER_PRIVATE_KEY ??
    JSON.parse(
      readFileSync(join(homedir(), ".near-credentials", "testnet", `${OWNER}.json`), "utf8"),
    ).private_key;
  return new Account(OWNER, provider, key);
}

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
