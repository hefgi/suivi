// suivi plugin for OpenCode.
//
// Plugin shape per https://opencode.ai/docs/plugins/:
//   export default async ({ project, client, $, directory, worktree }) => ({
//     event: async ({ event }) => { ... },
//   })
//
// Event payloads verified against the OpenCode SDK type definitions
// (packages/sdk/js/src/gen/types.gen.ts):
//   message.updated → { properties: { info: UserMessage | AssistantMessage } }
//                     both variants carry `sessionID` and `role`
//   session.idle    → { properties: { sessionID: string } }
//
// A turn is one user message → session idle. Firing `pre` per *user message*
// (not per session.created) gives real per-turn granularity and re-arms
// tracking after every idle; previously a session produced a single turn that
// ended at the first idle, so a 3-hour session counted as one buffered stamp.

import { spawn } from "child_process";

// Fire-and-forget: pipe the payload to suivi's stdin without a shell, a temp
// file, or blocking OpenCode's event loop. Errors (e.g. suivi not on PATH)
// are swallowed — tracking must never break the agent.
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

export default async ({ directory, worktree } = {}) => {
  // message.updated can fire repeatedly for the same message; only the first
  // sighting of a user message starts a turn. Bounded so a long-lived
  // process doesn't grow without limit (updates cluster at creation time, so
  // clearing rarely risks a duplicate).
  const seenUserMessages = new Set();

  return {
    event: async ({ event }) => {
      if (!event || typeof event.type !== "string") return;

      if (event.type === "message.updated") {
        const info = event.properties?.info;
        if (!info || info.role !== "user") return;
        const sid = info.sessionID;
        if (!sid) return;
        if (info.id) {
          if (seenUserMessages.has(info.id)) return;
          if (seenUserMessages.size > 2048) seenUserMessages.clear();
          seenUserMessages.add(info.id);
        }
        send(
          ["hook", "pre"],
          JSON.stringify({
            session_id: sid,
            cwd: directory ?? worktree ?? process.cwd(),
            agent: "opencode",
          }),
        );
      } else if (event.type === "session.idle") {
        const sid = event.properties?.sessionID;
        if (!sid) return;
        send(["hook", "stop"], JSON.stringify({ session_id: sid }));
      }
    },
  };
};
