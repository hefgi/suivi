# Supported agents

| Agent | Status | Granularity |
|-------|--------|-------------|
| Claude Code | ✅ v1 | Per-prompt |
| Codex | ✅ v1 | Per-prompt |
| OpenCode | ✅ v1 | Per session-idle cycle |
| Pi | ⚗️ experimental | Per session |

## Adding a new agent

1. Create `src/agents/<name>/mod.rs` and implement the `Agent` trait
2. Add hook template files under `src/agents/<name>/hooks/`
3. Register it in `all_agents()` in `src/agents/mod.rs`

The agent must provide a `session_id` in its hook payload — this is required for correct per-session tracking.

See [Contributing](contributing.md) for the full workflow.
