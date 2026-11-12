// Smoke test for the MCP server: connects like an AI client would, lists the
// tools, checks the budget, reads history and makes one payment attempt.
//   node mcp-smoke-test.js
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { SCAMMER } from "./lib.js";

const client = new Client({ name: "smoke-test", version: "1.0.0" });
await client.connect(new StdioClientTransport({
  command: process.execPath,
  args: [new URL("./mcp-server.js", import.meta.url).pathname],
  stderr: "ignore",
}));

const { tools } = await client.listTools();
console.log("tools:", tools.map((t) => t.name).join(", "));

const show = (title, r) => console.log(`\n--- ${title}${r.isError ? " (ERROR)" : ""}\n${r.content.map((c) => c.text).join("\n")}`);

show("leash_status", await client.callTool({ name: "leash_status", arguments: {} }));
show("leash_history", await client.callTool({ name: "leash_history", arguments: { limit: 3 } }));
// A payment the leash must refuse (scammer, not allowlisted / or frozen): costs only gas.
show("leash_pay → scammer", await client.callTool({
  name: "leash_pay",
  arguments: { receiver_id: SCAMMER, amount_near: "0.05", memo: "smoke test: should be blocked" },
}));
show("leash_pay with a bad amount", await client.callTool({
  name: "leash_pay",
  arguments: { receiver_id: SCAMMER, amount_near: "lots", memo: "invalid" },
}));

await client.close();
