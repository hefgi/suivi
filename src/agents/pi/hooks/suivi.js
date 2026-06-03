import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

export default {
  name: "suivi",
  beforeAgentStart(ctx) {
    const payload = JSON.stringify({
      session_id: ctx.sessionId,
      cwd: ctx.cwd,
    });
    const tmp = join(tmpdir(), `suivi-${ctx.sessionId}.json`);
    writeFileSync(tmp, payload);
    try {
      execSync(`suivi hook pre < "${tmp}"`, { stdio: "ignore" });
    } finally {
      try { unlinkSync(tmp); } catch (_) {}
    }
  },
  agentEnd(ctx) {
    const payload = JSON.stringify({
      session_id: ctx.sessionId,
      duration_ms: ctx.durationMs,
    });
    const tmp = join(tmpdir(), `suivi-stop-${ctx.sessionId}.json`);
    writeFileSync(tmp, payload);
    try {
      execSync(`suivi hook stop < "${tmp}"`, { stdio: "ignore" });
    } finally {
      try { unlinkSync(tmp); } catch (_) {}
    }
  },
};
