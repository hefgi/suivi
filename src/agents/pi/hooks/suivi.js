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

import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { basename, join } from "path";

function sessionIdFrom(ctx) {
  const file = ctx?.sessionManager?.getSessionFile?.();
  if (!file) return null;
  const b = basename(String(file));
  const stripped = b.replace(/\.[^.]+$/, "");
  return stripped || b || null;
}

function pipe(cmd, payload, tag) {
  const tmp = join(tmpdir(), `suivi-${tag}.json`);
  writeFileSync(tmp, payload);
  try {
    execSync(`${cmd} < "${tmp}"`, { stdio: "ignore" });
  } finally {
    try {
      unlinkSync(tmp);
    } catch (_) {}
  }
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
    pipe("suivi hook pre", payload, `pre-${sid}`);
  });

  pi.on("agent_end", async (_event, ctx) => {
    const sid = sessionIdFrom(ctx);
    if (!sid) return;
    const payload = JSON.stringify({ session_id: sid });
    pipe("suivi hook stop", payload, `stop-${sid}`);
  });
}
