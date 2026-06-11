// suivi plugin for OpenCode.
//
// Real plugin shape per https://opencode.ai/docs/plugins/:
//   export default async ({ project, client, $, directory, worktree }) => ({
//     event: async ({ event }) => { ... },
//   })
//
// The exact path to the session id inside the event payload is not pinned by
// the public docs; we use a defensive chain of fallbacks and silently drop
// the turn if none resolves (matches suivi's "session_id missing → drop" rule).

import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

function sessionIdFrom(event) {
  const p = event?.properties;
  return (
    p?.info?.id ??
    p?.sessionID ??
    p?.session_id ??
    p?.session?.id ??
    null
  );
}

function modelFrom(event) {
  return event?.properties?.info?.model ?? null;
}

function pipe(cmd, payload, tag) {
  const tmp = join(
    tmpdir(),
    `suivi-${tag}-${Date.now()}-${Math.random().toString(36).slice(2)}.json`,
  );
  writeFileSync(tmp, payload);
  try {
    execSync(`${cmd} < "${tmp}"`, { stdio: "ignore" });
  } finally {
    try {
      unlinkSync(tmp);
    } catch (_) {}
  }
}

export default async ({ directory, worktree } = {}) => ({
  event: async ({ event }) => {
    if (!event || typeof event.type !== "string") return;

    const sid = sessionIdFrom(event);
    if (!sid) return;

    if (event.type === "session.created") {
      const payload = JSON.stringify({
        session_id: sid,
        cwd: directory ?? worktree ?? process.cwd(),
        agent: "opencode",
        model: modelFrom(event),
      });
      pipe("suivi hook pre --agent opencode", payload, `pre-${sid}`);
    } else if (event.type === "session.idle") {
      const payload = JSON.stringify({ session_id: sid });
      pipe("suivi hook stop --agent opencode", payload, `stop-${sid}`);
    }
  },
});
