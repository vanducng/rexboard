// Runs src/hook/pi-extension.js against a stand-in for pi and a stand-in `rexboard`, in the
// environment this process was started with, and prints what it reported, one line per call:
// "hook pi SessionStart", "usage pi" and so on.
// Usage: node pi-extension-harness.mjs <extension.js> <ctx.mode> <ctx.hasUI: true|false>
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [source, mode, hasUI] = process.argv.slice(2);
const dir = mkdtempSync(join(tmpdir(), "rbpi"));
const log = join(dir, "calls");
const fake = join(dir, "rexboard");
writeFileSync(fake, `#!/bin/sh\nprintf '%s ' "$@" >>${log}\ncat >>${log}\necho >>${log}\n`);
chmodSync(fake, 0o755);
const code = readFileSync(source, "utf8").replace("__REXBOARD__", JSON.stringify(fake));
const file = join(dir, "extension.mjs");
writeFileSync(file, code);

const handlers = {};
const pi = { on: (name, fn) => (handlers[name] = fn), getThinkingLevel: () => "high" };
(await import(pathToFileURL(file))).default(pi);

const ctx = { mode, hasUI: hasUI === "true", model: { id: "model-x" } };
const fire = (name, event) => handlers[name]?.(event, ctx);
await fire("session_start", { reason: "startup" });
await fire("agent_start", {});
await fire("message_end", {
  message: { role: "assistant", content: [{ type: "text", text: "all done" }] },
});
await fire("turn_end", {});
await fire("agent_settled", {});
await fire("session_shutdown", { reason: "quit" });
await new Promise((resolve) => setTimeout(resolve, 1000));

let calls = "";
try {
  calls = readFileSync(log, "utf8");
} catch {}
for (const line of calls.split("\n").filter(Boolean)) {
  const at = line.indexOf("{");
  const args = line.slice(0, at).trim();
  const event = JSON.parse(line.slice(at)).hook_event_name;
  console.log(event ? `${args} ${event}` : args);
}
