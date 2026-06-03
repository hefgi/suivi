import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

export default {
  name: "suivi",
  onSessionStart(ctx) {
    const payload = JSON.stringify({
      session_id: ctx.sessionID,
      cwd: ctx.cwd,
      model: ctx.model,
    });
    const tmp = join(tmpdir(), `suivi-${ctx.sessionID}.json`);
    writeFileSync(tmp, payload);
    try {
      execSync(`suivi hook pre < "${tmp}"`, { stdio: "ignore" });
    } finally {
      try { unlinkSync(tmp); } catch (_) {}
    }
  },
  onSessionEnd(ctx) {
    const payload = JSON.stringify({
      session_id: ctx.sessionID,
      duration_ms: ctx.durationMs,
    });
    const tmp = join(tmpdir(), `suivi-stop-${ctx.sessionID}.json`);
    writeFileSync(tmp, payload);
    try {
      execSync(`suivi hook stop < "${tmp}"`, { stdio: "ignore" });
    } finally {
      try { unlinkSync(tmp); } catch (_) {}
    }
  },
};
