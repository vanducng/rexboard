// rexboard's pi extension, written by `rexboard hooks install`; an install replaces it.
// It reports pi's status to Rex through `rexboard hook pi`, as Claude Code's and Codex's hooks
// do: working while it runs, done with its final message when it settles, error when the model
// fails. At each turn's end it keeps what the turn used with `rexboard usage pi`.
import { spawn } from "node:child_process";

const REXBOARD = __REXBOARD__;

// The same gate as the other harnesses' hooks: inside tmux or a Rex pane.
export function reporting(env) {
  return Boolean(env.TMUX || env.REX_BLOCK || /^(tmux|screen)/.test(env.TERM ?? ""));
}

export function textOf(message) {
  return (message?.content ?? [])
    .filter((part) => part?.type === "text")
    .map((part) => part.text ?? "")
    .join("");
}

// What agent_settled reports: the run's last assistant text, or the model's error.
export function settled(last, failure) {
  return failure
    ? { hook_event_name: "StopFailure", last_assistant_message: failure }
    : { hook_event_name: "Stop", last_assistant_message: last };
}

// Each call waits for the one before it, at most this long, so a quick `working` never lands
// before the `idle` sent ahead of it. pi awaits a run's last report: `pi -p` exits right after
// it, and rexboard finds pi's pane through pi's own process, so pi must still be running.
const WAIT = 2000;

function run(args, payload) {
  if (!reporting(process.env)) return Promise.resolve();
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn("/bin/sh", ["-c", `exec ${REXBOARD} "$@"`, "rexboard", ...args], {
        stdio: ["pipe", "ignore", "ignore"],
      });
    } catch {
      return resolve();
    }
    const done = () => {
      clearTimeout(timer);
      child.unref();
      resolve();
    };
    const timer = setTimeout(done, WAIT);
    child.on("error", done);
    child.on("exit", done);
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
  });
}

export default function rexboard(pi) {
  let last = "";
  let failure;
  let sent;
  let spent = 0;
  let queue = Promise.resolve();
  const send = (args, payload) => (queue = queue.then(() => run(args, payload)));
  const hook = (event) => send(["hook", "pi"], event);

  pi.on("session_start", async () => {
    hook({ hook_event_name: "SessionStart" });
  });

  pi.on("agent_start", async () => {
    last = "";
    failure = undefined;
    hook({ hook_event_name: "UserPromptSubmit" });
  });

  pi.on("message_end", async (event) => {
    const message = event?.message;
    if (message?.role !== "assistant") return;
    last = textOf(message) || last;
    sent = message.usage ?? sent;
    spent += message.usage?.cost?.total ?? 0;
    if (message.stopReason === "error") failure = message.errorMessage || "the model failed";
  });

  // The context is final only at a turn's end; TOK is the last message's prompt side.
  pi.on("turn_end", async (_event, ctx) => {
    send(["usage", "pi"], {
      model: ctx?.model?.id,
      effort: pi.getThinkingLevel?.(),
      context: ctx?.getContextUsage?.(),
      usage: sent,
      cost: spent,
    });
  });

  pi.on("agent_settled", async () => hook(settled(last, failure)));

  // Without a UI (`pi -p`) the session ends with its one run, whose result should stay up.
  pi.on("session_shutdown", async (_event, ctx) => {
    if (ctx?.hasUI) await hook({ hook_event_name: "SessionEnd" });
  });
}
