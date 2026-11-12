// Generates the agent's key pair and stores it in agent/.env (git-ignored).
// The public key is then registered on-chain as a FUNCTION-CALL key that may
// only call `pay` on the leash contract (see scripts/setup-agent-key.sh).
import { KeyPair } from "near-api-js";
import { existsSync, writeFileSync } from "node:fs";

const envPath = new URL("./.env", import.meta.url);
if (existsSync(envPath)) {
  console.error("agent/.env already exists, so no new key was generated. Delete it first if you really want a new key.");
  process.exit(1);
}

const kp = KeyPair.fromRandom("ed25519");
writeFileSync(envPath, `AGENT_PRIVATE_KEY=${kp.toString()}\n`, { mode: 0o600 });
console.log(kp.getPublicKey().toString());
