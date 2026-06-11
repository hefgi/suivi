# Supported agents

| Agent | Status | Granularity |
|-------|--------|-------------|
| Claude Code | ✅ v1 | Per-prompt (`UserPromptSubmit` + `Stop`) |
| Codex | ✅ v1 | Per-prompt (`UserPromptSubmit` + `Stop`) |
| OpenCode | ✅ v1 | Per-prompt (user `message.updated` + `session.idle`) |
| Pi | ⚗️ experimental | Per agent run (`before_agent_start` + `agent_end`) |

## Adding a new agent

1. Create `src/agents/<name>/mod.rs` and implement the `Agent` trait (including `is_installed`, so `suivi init` only targets machines that have the agent)
2. Add hook template files under `src/agents/<name>/hooks/`
3. Register it in `all_agents()` in `src/agents/mod.rs`

The agent must provide a `session_id` in its hook payload — this is required for correct per-session tracking.

See [Contributing](contributing.md) for the full workflow.
