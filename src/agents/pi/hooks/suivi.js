// suivi extension for the Pi coding agent.
//
// Real extension shape per
// https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/extensions.md:
//   export default function (pi) {
//     pi.on("before_agent_start", async (event, ctx) => { ... });
//     pi.on("agent_end",         async (event, ctx) => { ... });
//   }
//
// Session id is obtained via ctx.sessionManager.getSessionFile() (returns a
// path or undefined for ephemeral sessions). We use the basename without
// extension as the opaque suivi session_id. If unavailable, the turn is
// silently dropped (matches suivi's contract).

import { spawn } from "child_process";
import { basename } from "path";

function sessionIdFrom(ctx) {
  const file = ctx?.sessionManager?.getSessionFile?.();
  if (!file) return null;
  const b = basename(String(file));
  const stripped = b.replace(/\.[^.]+$/, "");
  return stripped || b || null;
}

// Fire-and-forget: pipe the payload to suivi's stdin without a shell, a temp
// file, or blocking Pi's event loop. Errors (e.g. suivi not on PATH) are
// swallowed — tracking must never break the agent.
function send(args, payload) {
  try {
    const child = spawn("suivi", args, {
      stdio: ["pipe", "ignore", "ignore"],
    });
    child.on("error", () => {});
    child.stdin.on("error", () => {});
    child.stdin.write(payload);
    child.stdin.end();
  } catch (_) {}
}

export default function (pi) {
  pi.on("before_agent_start", async (_event, ctx) => {
    const sid = sessionIdFrom(ctx);
    if (!sid) return;
    const payload = JSON.stringify({
      session_id: sid,
      cwd: ctx?.cwd ?? process.cwd(),
      agent: "pi",
    });
    send(["hook", "pre"], payload);
  });

  pi.on("agent_end", async (_event, ctx) => {
    const sid = sessionIdFrom(ctx);
    if (!sid) return;
    const payload = JSON.stringify({ session_id: sid });
    send(["hook", "stop"], payload);
  });
}
