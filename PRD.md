# PRD — suivi

## Overview

A Rust CLI tool that tracks time spent working with AI coding agents (Claude Code, Codex, OpenCode, Pi, and others) across multiple projects. Built as a single distributable binary, installable via Homebrew or Cargo. Inspired by RTK's analytics UX.

---

## Problem

Developers running multiple AI agent sessions across multiple projects simultaneously have no visibility into where their time actually goes. There is no existing tool that:
- Captures per-turn agent activity and maps it to a project
- Accounts for human think time (writing prompts, reading outputs)
- Aggregates across heterogeneous agents (Claude Code, Codex, OpenCode, Pi)
- Presents project-level and cross-project time analytics in both wall-clock and accumulated time

---

## Goals

- Track time at the per-agent-turn granularity across all CLI-based AI agents
- Attribute each turn to a tracked project via CWD → config path matching
- Surface analytics: total time, by project, by agent, by model, by day
- Show both wall-clock time (real elapsed) and accumulated time (agent-hours invested)
- Provide a CLI experience on par with RTK's `gain` command
- Zero friction: hook-based, no manual session management

---

## Non-Goals (v1)

- Web UI or dashboard
- Per-agent or per-project buffer time configuration
- Support for non-CLI agents (web UIs, API calls)
- Time tracking for non-agent terminal activity
- Billing / cost tracking

---

## Core Concepts

### Turn

The atomic unit of tracking. A turn is one complete agent exchange:
- Starts when the user submits a prompt (`UserPromptSubmit` or equivalent hook fires)
- Ends when the agent delivers its full response (`Stop` hook fires)

Each turn is scoped to a `session_id` provided by the agent. Turns without a session ID are silently dropped.

### Session

A continuous sequence of turns sharing the same `session_id`, as provided by the calling agent. Multiple sessions can run concurrently on the same project (e.g. two Claude Code windows in `accounting/`) or across different projects.

### Wall-clock Time

Real elapsed time a project occupied the user's day. Computed by taking the union of all turn intervals (with buffers applied) across all sessions for a project, then summing the merged non-overlapping ranges. Five parallel 1-minute sessions = 1 minute of wall-clock time.

### Accumulated Time

Total agent-hours invested in a project. Sum of all individual turn effective durations across all sessions. Five parallel 1-minute sessions = 5 minutes of accumulated time. Reflects total cognitive effort regardless of parallelism.

### Effective Duration

The time charged per turn:

```
Let B = human_buffer_secs (default 300)
Let T = agent_thinking_secs
Let G = gap between this turn's Stop and the next turn's Pre within the same session

if G < B * 2:
    effective_duration = G + T        -- gap is the real human time, use it as-is
else:
    effective_duration = B + T + B    -- no next turn soon, apply full buffers on both sides
```

Example: agent responds, user re-prompts 30 seconds later → charge 30s + T, not 10 minutes + T.

**Computation strategy**: `hook stop` writes `ended_at` and sets `effective_duration = B + T + B` as a best-guess. When the next `hook pre` fires in the same session, it corrects the previous turn's `effective_duration` if the gap was short. At most 2 DB writes per turn. No daemon required.

### Project

A directory path registered in the config. On each turn, the agent's CWD is matched against tracked paths using **nearest-ancestor matching** — the deepest tracked path that is a prefix of the CWD wins. Glob patterns (`Waver-Labs/*/`) are expanded at config load time into individual tracked entries.

---

## Configuration

**Location**: `~/.config/suivi/config.toml`

```toml
[tracking]
human_buffer_secs = 300  # 5 minutes, applied before and after each turn
retention_days = 365     # turns older than this are pruned automatically

[[projects]]
path = "~/Desktop/Hefgi/tracker-code"

[[projects]]
path = "~/Desktop/Hefgi/agent-tilt/ecluse"

[[projects]]
path = "~/Desktop/Hefgi/Waver-Labs/*"

[[projects]]
path = "~/Desktop/Hefgi/Rubbr"
name = "Rubbr"  # optional override, defaults to directory name otherwise

[[projects]]
path = "~/Desktop/Hefgi/Enzyme"
```

Project names are derived from the last path segment by default (e.g. `tracker-code`, `ecluse`, `Rubbr`). The optional `name` field overrides this. Glob paths with `*` are expanded at startup — each matched subdirectory becomes an independent project named by its own directory name.

Pruning runs automatically on `suivi init` re-runs and on `suivi stats`. Turns older than `retention_days` are deleted.

---

## Agent Modularity

`suivi` is **agent-agnostic by design**. The v1 release ships with support for Claude Code, Codex, OpenCode, and Pi, but the architecture makes it trivial for contributors to add new agents without touching core logic.

### Module Structure

Each supported agent lives in its own module under `src/agents/<agent-name>/`:

```
src/
  agents/
    mod.rs          -- Agent trait definition + registry
    claude_code/
      mod.rs        -- ClaudeCode agent implementation
      hooks/
        user_prompt_submit.json    -- UserPromptSubmit hook template
        stop.json                  -- Stop hook template
    codex/
      mod.rs
      hooks/
        pre.json
        stop.json
    opencode/
      mod.rs
      hooks/
        suivi.js    -- JS plugin installed to ~/.config/opencode/plugins/
    pi/
      mod.rs
      hooks/
        suivi.js    -- JS extension installed to ~/.pi/agent/extensions/
```

Hook template files are embedded into the binary at build time via `include_str!()` / `rust-embed`, so the distributed binary has zero external file dependencies.

### The `Agent` Trait

Every agent implements a single trait:

```rust
pub trait Agent: Send + Sync {
    /// Unique identifier stored in the database (e.g. "claude-code")
    fn id(&self) -> &'static str;

    /// Human-readable display name (e.g. "Claude Code")
    fn display_name(&self) -> &'static str;

    /// Detect if this agent is calling suivi based on env vars / process ancestry
    fn detect(&self, env: &Env) -> bool;

    /// Parse agent-specific fields from the hook payload.
    /// Returns None if session_id is missing — turn will be silently dropped.
    fn parse_payload(&self, raw: &str) -> Option<AgentPayload>;

    /// Return the hook config snippet to inject during `suivi init`
    fn hook_templates(&self) -> HookTemplates;
}
```

`AgentPayload` is a common struct with a required `session_id: String` and optional fields (`model`, etc.). If `session_id` is absent in the payload, `parse_payload` returns `None` and the turn is dropped.

### Agent Registry

Agents are registered in a static list in `src/agents/mod.rs`:

```rust
pub fn all_agents() -> Vec<Box<dyn Agent>> {
    vec![
        Box::new(ClaudeCode),
        Box::new(Codex),
        Box::new(OpenCode),
        Box::new(Pi),
    ]
}
```

At hook time, `suivi` iterates `all_agents()` and calls `detect()` on each to identify the caller. The first match wins. If no agent matches, the turn is dropped.

### Adding a New Agent (Contributor Guide)

To add support for a new agent:

1. Create `src/agents/<name>/mod.rs` and implement the `Agent` trait
2. Add hook template files under `src/agents/<name>/hooks/`
3. Register it in `all_agents()` in `src/agents/mod.rs`
4. That's it — detection, init, and analytics all pick it up automatically

No changes to core tracking, storage, or analytics code required.

---

## Hook Integration

The tool integrates via each agent's hook system. Hook template files are stored per-agent and injected during `suivi init`.

### Claude Code

Registers two hooks in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{ "type": "command", "command": "suivi hook pre" }]
    }],
    "Stop": [{
      "hooks": [{ "type": "command", "command": "suivi hook stop" }]
    }]
  }
}
```

### Codex

Config location: `~/.codex/hooks.json`

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{ "type": "command", "command": "suivi hook pre" }]
    }],
    "Stop": [{
      "hooks": [{ "type": "command", "command": "suivi hook stop" }]
    }]
  }
}
```

Payload includes `session_id`, `cwd`, `model`, `hook_event_name` via stdin JSON. Identical protocol to Claude Code.

### Pi

Pi uses a **JavaScript/TypeScript extension system**, not shell hooks. Extensions are auto-discovered from `~/.pi/agent/extensions/*.js` (global) or `.pi/extensions/*.js` (project-local).

The extension exports a default function receiving the `pi: ExtensionAPI` object. External commands are shelled out via Node's `child_process.execSync`, piping JSON to `suivi hook pre|stop` through a temp file.

```js
// ~/.pi/agent/extensions/suivi.js
import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { basename, join } from "path";

const sessionIdFrom = (ctx) => {
  const file = ctx?.sessionManager?.getSessionFile?.();
  if (!file) return null;
  return basename(String(file)).replace(/\.[^.]+$/, "") || null;
};

const pipe = (cmd, payload, tag) => {
  const tmp = join(tmpdir(), `suivi-${tag}.json`);
  writeFileSync(tmp, payload);
  try { execSync(`${cmd} < "${tmp}"`, { stdio: "ignore" }); }
  finally { try { unlinkSync(tmp); } catch (_) {} }
};

export default function (pi) {
  pi.on("before_agent_start", async (_event, ctx) => {
    const sid = sessionIdFrom(ctx);
    if (!sid) return;
    pipe("suivi hook pre", JSON.stringify({
      session_id: sid,
      cwd: ctx?.cwd ?? process.cwd(),
      agent: "pi",
    }), `pre-${sid}`);
  });

  pi.on("agent_end", async (_event, ctx) => {
    const sid = sessionIdFrom(ctx);
    if (!sid) return;
    pipe("suivi hook stop", JSON.stringify({ session_id: sid }), `stop-${sid}`);
  });
}
```

`suivi init` installs this file automatically when Pi is detected.

**Note**: Pi exposes the session id as a file path via `ctx.sessionManager.getSessionFile()` (or `undefined` for ephemeral sessions). suivi uses the basename without extension as the opaque session id. Ephemeral sessions are silently dropped.

### OpenCode

OpenCode uses a **JavaScript/TypeScript plugin system**. Plugins are registered at `~/.config/opencode/plugins/suivi.js` (global) or `.opencode/plugins/suivi.js` (project-local).

The plugin exports an async default function receiving a destructured context (`{ project, client, $, directory, worktree }`) and returns an object of named event handlers. We subscribe to the generic `event` handler and switch on `event.type`.

```js
// ~/.config/opencode/plugins/suivi.js
import { execSync } from "child_process";
import { writeFileSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

const sessionIdFrom = (event) => {
  const p = event?.properties;
  return p?.info?.id ?? p?.sessionID ?? p?.session_id ?? p?.session?.id ?? null;
};

const pipe = (cmd, payload, tag) => {
  const tmp = join(tmpdir(), `suivi-${tag}-${Date.now()}.json`);
  writeFileSync(tmp, payload);
  try { execSync(`${cmd} < "${tmp}"`, { stdio: "ignore" }); }
  finally { try { unlinkSync(tmp); } catch (_) {} }
};

export default async ({ directory, worktree } = {}) => ({
  event: async ({ event }) => {
    const sid = sessionIdFrom(event);
    if (!sid) return;
    if (event.type === "session.created") {
      pipe("suivi hook pre", JSON.stringify({
        session_id: sid,
        cwd: directory ?? worktree ?? process.cwd(),
        agent: "opencode",
        model: event?.properties?.info?.model ?? null,
      }), `pre-${sid}`);
    } else if (event.type === "session.idle") {
      pipe("suivi hook stop", JSON.stringify({ session_id: sid }), `stop-${sid}`);
    }
  },
});
```

`suivi init` installs this file automatically when OpenCode is detected.

**Note**: OpenCode has no `UserPromptSubmit` equivalent. The closest events are `session.created` and `session.idle`. Turn granularity for OpenCode is coarser — tracked per session-idle cycle, not per individual prompt. This is a known limitation tracked in Open Questions. The exact JSON path to the session id inside an event payload is not pinned in OpenCode's public docs; suivi uses a defensive fallback chain (`event.properties.info.id` first) and silently drops turns when none resolves.

### Hook Payloads

- `suivi hook pre`: fires on `UserPromptSubmit` (once per user turn, not per tool call). Reads JSON from stdin, detects calling agent, extracts `session_id` (drops turn if missing), records turn start. Also corrects the previous turn's `effective_duration` in the same session if the gap was short.
- `suivi hook stop`: records `ended_at`, writes initial `effective_duration = B + T + B` as best-guess pending correction by next `hook pre`.

---

## Data Storage

**Engine**: SQLite
**Location**: `~/.local/share/suivi/history.db`

### Schema

```sql
CREATE TABLE turns (
  id                      INTEGER PRIMARY KEY,
  session_id              TEXT NOT NULL,        -- agent-provided session identifier
  started_at              TEXT NOT NULL,        -- RFC3339 UTC
  ended_at                TEXT,                 -- NULL until Stop fires
  project_path            TEXT,                 -- matched tracked path, NULL if untracked
  project_name            TEXT,                 -- resolved display name
  cwd                     TEXT NOT NULL,        -- raw CWD at turn start
  agent                   TEXT NOT NULL,        -- "claude-code" | "codex" | "opencode" | "pi"
  model                   TEXT,                 -- e.g. "claude-sonnet-4-6", NULL if unknown
  agent_duration_secs     REAL,                 -- wall-clock agent thinking time (ended_at - started_at)
  effective_duration_secs REAL                  -- agent_duration + applied buffers, corrected by next hook pre
);

CREATE INDEX idx_started_at ON turns(started_at);
CREATE INDEX idx_session_id ON turns(session_id, started_at);
CREATE INDEX idx_project_path ON turns(project_path, started_at);
CREATE INDEX idx_agent ON turns(agent, started_at);
```

Incomplete turns (no `Stop` fired, `ended_at` is NULL) older than 2 hours are excluded from analytics.

---

## CLI Commands

### `suivi stats`

Summary view showing both wall-clock and accumulated time, top projects, top agents.

```
suivi — Summary
─────────────────────────────────────────────────────

  Today     wall-clock  2h 14m   accumulated  3h 41m
  This week wall-clock 11h 03m   accumulated 18h 22m
  All time  wall-clock 43h 22m   accumulated 71h 15m

  Top projects (this week)
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Project        Wall-clock  Accumulated  Turns  Sessions
  accounting       4h 12m      6h 45m      312   claude-code ×2  pi ×3  │ 12 total
  tracker-code     3h 01m      3h 01m      198   claude-code ×1          │  4 total
  Enzyme           2h 19m      2h 19m      143   codex ×1                │  3 total
  Rubbr            1h 31m      1h 31m       89   claude-code ×1          │  2 total

  Top agents (this week)
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  claude-code       8h 44m  ████████████████░░░░  79%
  pi                2h 19m  ████░░░░░░░░░░░░░░░░  21%
```

### `suivi stats --graph`

ASCII bar graph showing both wall-clock and accumulated time per day, last 30 days. Two lines per day so parallelism is visually apparent.

```
Daily activity — last 30 days
Jun 01  wall-clock  ████████████░░░░░░░░  4h 12m
        accumulated ██████████████████░░  6h 45m
Jun 02  wall-clock  ██████░░░░░░░░░░░░░░  2h 01m
        accumulated ██████░░░░░░░░░░░░░░  2h 01m
```

### `suivi stats --daily`

Day-by-day breakdown table: date, wall-clock, accumulated, turns, top project, top agent.

### `suivi stats --history`

Recent turns list: timestamp, project, agent, model, agent duration, effective duration. Defaults to today. Use time flags to widen the window (`--week`, `--month`, `--all`, etc.).

### `suivi stats --projects`

Cross-project comparison table.

The Sessions column shows peak concurrency (max simultaneous sessions at any point in the window) and total distinct sessions.

```
suivi — Project comparison
──────────────────────────────────────────────────────────────────────────────────────────
Project        Wall-clock  Accumulated   Turns  Sessions
──────────────────────────────────────────────────────────────────────────────────────────
accounting       4h 12m      6h 45m       312   claude-code ×2  pi ×3  │ 12 total
tracker-code     3h 01m      3h 01m       198   claude-code ×1          │  4 total
Enzyme           2h 19m      2h 19m       143   codex ×1                │  3 total
Rubbr            1h 31m      1h 31m        89   claude-code ×1          │  2 total
notion           1h 12m      1h 12m        34   opencode ×1             │  1 total
──────────────────────────────────────────────────────────────────────────────────────────
Total           11h 03m     18h 22m       776
```

### `suivi stats --project <name>`

Scoped view for one project: wall-clock vs accumulated graph, agent time split, model breakdown, recent history.

### `suivi stats --agent <name>`

Scoped view for one agent: time graph, project time split, model breakdown.

### `suivi stats --all --format json`

Full data export as JSON. Useful for external dashboards.

### `suivi hook pre`

Internal — called by agent hooks on turn start. Not intended for direct user use.

### `suivi hook stop`

Internal — called by agent hooks on turn end. Not intended for direct user use.

### `suivi init`

First-time setup wizard and re-sync command:
- If config does not exist: creates `~/.config/suivi/config.toml` interactively, asks for project paths, then proceeds to hook registration
- If config already exists: prints "Config already exists at ~/.config/suivi/config.toml, skipping" and proceeds directly to hook sync
- Always: detects installed agents, re-syncs hooks, never modifies an existing config file

### `suivi doctor`

Database maintenance. Reports counts of stale and beyond-retention turns; optionally prunes them.

- *(default)* or `--check`: runs `PRAGMA integrity_check` against the SQLite database and reports `ok` or the error. Always prints DB status (stale-open turns >2h, turns older than `retention_days`).
- `--prune`: deletes stale (`ended_at IS NULL` and started >2h ago) turns plus all turns older than `retention_days`. Reports the deletion counts. Skips the integrity check when `--prune` is the only flag, for speed.

Run periodically if you suspect DB drift, or wire it into your shell startup. Pruning also runs automatically on `suivi init` re-runs and on every `suivi stats` invocation, so explicit `suivi doctor --prune` is rarely needed.

### `suivi status`

Shows:
- Hook installation health per agent (Ok / Outdated / Missing)
- Config path and database path
- Number of tracked projects
- Untracked activity (last 7 days): count of turns with no project match, top untracked CWDs

```
suivi — Status
──────────────────────────────────────────
  Config     ~/.config/suivi/config.toml
  Database   ~/.local/share/suivi/history.db
  Projects   8 tracked

  Hooks
  claude-code   Ok
  codex         Ok
  opencode      Missing
  pi            Ok

  Untracked activity (last 7 days)
  43 turns not attributed to any project
  Top untracked paths:
    ~/Desktop/Hefgi/new-project/   31 turns
    ~/Desktop/scratch/             12 turns
```

---

## Time Flags (shared across stats commands)

| Flag | Window |
|------|--------|
| *(default)* | current day + last 7 days + all time summary |
| `--today` | today only |
| `--week` | last 7 rolling days |
| `--month` | last 30 rolling days |
| `--all` | all time |
| `--since <date>` | from date (YYYY-MM-DD) |

Note: `--week` and `--month` are rolling windows (last N days), not calendar week/month boundaries.

---

## Output Flags

| Flag | Effect |
|------|--------|
| `--format json` | JSON output |
| `--format csv` | CSV output |
| *(default)* | styled terminal output, TTY-aware |

---

## Distribution

- **Homebrew**: tap at `hefgi/homebrew-tap` (https://github.com/hefgi/homebrew-tap), formula auto-updated on release
- **Cargo**: published to crates.io on release
- **GitHub Releases**: pre-built binaries for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`

## CI / CD

Three GitHub Actions workflows, mirroring the ecluse setup:

### `ci.yml` — runs on every push and PR
- `cargo build`
- `cargo clippy -- -D warnings`
- `cargo fmt --check`
- `cargo test`
- Matrix: `ubuntu-latest` + `macos-latest`

### `release.yml` — triggers on `v*.*.*` tags
1. **Build** — cross-compiles for all 4 targets in parallel
2. **Release** — creates GitHub release with binaries, extracts changelog section from `CHANGELOG.md`
3. **publish-crates** — publishes to crates.io (requires `CARGO_REGISTRY_TOKEN` secret)
4. **update-homebrew** — updates `Formula/suivi.rb` in `hefgi/homebrew-tap` (requires `HOMEBREW_TAP_TOKEN` secret)

### `docs.yml` — triggers on pushes to `docs/**`
- Builds mdBook and deploys to GitHub Pages at `hefgi.github.io/suivi`
- Generates `llms.txt` and `llms-full.txt` for LLM consumption

## Documentation

Built with **mdBook**, deployed via GitHub Pages to `https://hefgi.github.io/suivi`.

Source lives in `docs/src/`. Pages:
- Introduction
- Install
- Quick start
- Configuration
- Commands
- Agents (supported agents + contributor guide)
- How time is measured (wall-clock vs accumulated, buffer cap)
- Contributing

Includes `llms.txt` and `llms-full.txt` generated at build time for LLM-friendly consumption of the docs.

---

## Rendering Stack (Rust)

Consistent with RTK's approach:
- `clap` (derive) — argument parsing
- `colored` — ANSI terminal colors, TTY-aware
- `rusqlite` — SQLite via bundled feature (zero system dep)
- `toml` — config parsing
- `serde` / `serde_json` — serialization
- `dirs` — XDG-compliant config/data paths
- `glob` — path pattern expansion
- ASCII graphs and tables rendered manually (Unicode box-drawing chars, no tui/ratatui)

---

## Quality & Testing

### Unit Tests

Every pure logic module has unit tests covering the happy path and edge cases:

- **Buffer cap logic** — verify `effective_duration` for: gap < B*2, gap > B*2, no next turn, zero-length gap
- **Project matching** — nearest-ancestor matching with overlapping paths, glob expansion, unmatched CWDs
- **Wall-clock interval merging** — overlapping turns, adjacent turns, single turn, empty set
- **Accumulated time** — sum across multiple sessions including concurrent ones
- **Config parsing** — valid TOML, missing fields defaulting correctly, invalid paths
- **Agent payload parsing** — valid payload, missing `session_id` returns `None`, unknown fields ignored

Run with: `cargo test`

### Integration Tests

Test the full `hook pre` → `hook stop` → analytics pipeline against a real SQLite DB:

- Single session: one turn, correct effective duration
- Single session: two back-to-back turns, buffer cap correction applied
- Two concurrent sessions on the same project: wall-clock deduplication correct, accumulated double-counts correctly
- Two sessions on different projects: no cross-contamination
- Turn with no `Stop` fired: excluded from analytics after 2-hour stale threshold
- Untracked CWD: stored with `project_path = NULL`, surfaced in `suivi status`
- Retention pruning: turns older than `retention_days` deleted, newer ones preserved

### CLI Snapshot Tests

Capture the rendered output of each `suivi stats` variant and assert it matches expected output. Uses a fixed seed DB so output is deterministic. Catches regressions in formatting, column alignment, and ASCII graph rendering.

Commands covered: `stats`, `stats --graph`, `stats --daily`, `stats --history`, `stats --projects`, `stats --project <name>`, `stats --agent <name>`, `status`

### Agent Hook Contract Tests

Each agent module has a test that:
1. Feeds a sample hook payload (captured from the real agent) into `parse_payload`
2. Asserts `session_id`, `cwd`, `agent`, `model` are extracted correctly
3. Asserts missing `session_id` returns `None`

This guards against payload format changes breaking agent support silently.

### Manual Testing Checklist (pre-release)

- [ ] `suivi init` on a clean machine creates config and registers hooks for all detected agents
- [ ] `suivi init` re-run on existing config prints skip message and re-syncs hooks only
- [ ] Open two Claude Code sessions in the same project simultaneously — verify wall-clock < accumulated in `suivi stats`
- [ ] Open Claude Code in an untracked directory — verify it appears in `suivi status` untracked section
- [ ] Run `suivi stats --all --format json` and validate JSON structure
- [ ] Verify `suivi hook pre` is a no-op (silent exit 0) when called outside an agent session

---

## Open Questions (deferred)

- Per-agent or per-project buffer time configuration (post-v1)
- Support for `PostToolUse` in addition to `Stop` for finer turn granularity (post-v1)
- Idle detection beyond the buffer cap (post-v1)
- Codex confirmed to provide `session_id` in hook payload — same protocol as Claude Code
- Pi session ID: resolved via `ctx.sessionManager.getSessionFile()` (file path). suivi uses the basename (without extension) as the opaque session id. Ephemeral (`undefined`) sessions are silently dropped.
- OpenCode session ID: extracted from event payload via defensive chain `event.properties.info.id ?? event.properties.sessionID ?? event.properties.session_id ?? event.properties.session?.id`. Pin to a single canonical path once OpenCode's plugin event types are stable.
- OpenCode limitation: no `UserPromptSubmit` equivalent — tracking is per session-idle cycle, not per prompt. Consider whether to accept this coarser granularity or find a workaround (post-v1)
