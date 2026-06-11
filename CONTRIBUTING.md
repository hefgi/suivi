# Contributing to suivi

Issues and PRs are welcome. Check the [open issues](https://github.com/hefgi/suivi/issues) for ideas, and see the full docs at [hefgi.github.io/suivi](https://hefgi.github.io/suivi/).

## Dev workflow

```bash
cargo build
cargo test
cargo clippy -- -D warnings   # CI fails on any warning
cargo fmt --check             # CI checks formatting
```

CI runs all four on `ubuntu-latest` and `macos-latest` for every push and PR.

## Adding a new agent

Adding support for a new agent takes ~30 lines and no changes to core tracking, storage, or analytics code:

1. Create `src/agents/<name>/mod.rs` and implement the `Agent` trait
   (`id`, `display_name`, `detect`, `parse_payload`, `hook_templates`, `is_installed`)
2. Add hook template files under `src/agents/<name>/hooks/` — embedded into the
   binary via `include_str!`, so the distributed binary stays self-contained
3. Register it in `all_agents()` in `src/agents/mod.rs`

Two contracts to respect:

- The agent must provide a `session_id` in its hook payload — required for
  per-session tracking. Payloads without one are silently dropped.
- The installed hook command should pass the agent's identity explicitly
  (`suivi hook pre --agent <id>`), so attribution doesn't depend on
  environment sniffing.

Please verify hook locations and payload shapes against the agent's actual
documentation (or a captured real payload) rather than assuming another
agent's protocol — and say in the PR how you verified them.

## Project layout

```
src/
  agents/       one module per supported agent + the Agent trait
  analytics/    stats, graph, daily, history, per-project/agent views
  commands/     init, status, doctor
  hooks/        `suivi hook pre` / `suivi hook stop` handlers
  db.rs         SQLite schema and queries
  config.rs     config.toml parsing and project path matching
docs/           mdBook sources for the documentation site
```
