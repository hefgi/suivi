# Contributing

Issues and PRs are welcome. Check the [open issues](https://github.com/hefgi/suivi/issues) for ideas.

## Adding a new agent

Adding support for a new agent takes ~30 lines:

1. Create `src/agents/<name>/mod.rs`
2. Implement the `Agent` trait (`id`, `display_name`, `detect`, `parse_payload`, `hook_templates`)
3. Add hook template files under `src/agents/<name>/hooks/`
4. Register in `all_agents()` in `src/agents/mod.rs`

The agent must provide a `session_id` in its hook payload — this is required for correct per-session tracking.

## Dev workflow

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
```
