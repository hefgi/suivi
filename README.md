# suivi

Track time spent working with AI coding agents across multiple projects.

`suivi` hooks into Claude Code, Codex, OpenCode, Pi, and other CLI-based agents to measure how much time you spend on each project — broken down by agent, model, and day. Shows both wall-clock time (real elapsed) and accumulated time (total agent-hours invested).

## Install

```bash
brew install hefgi/tap/suivi
```

Or via Cargo:

```bash
cargo install suivi
```

## Setup

```bash
suivi init
```

This creates your config at `~/.config/suivi/config.toml`, registers hooks for all detected agents, and sets up project tracking.

## Usage

```bash
suivi stats                    # summary: today, this week, all time
suivi stats --graph            # daily graph (last 30 days)
suivi stats --daily            # day-by-day breakdown
suivi stats --history          # recent turns (today by default)
suivi stats --projects         # cross-project comparison
suivi stats --project <name>   # drill into one project
suivi stats --agent <name>     # drill into one agent
suivi status                   # hook health + untracked activity
```

## Configuration

`~/.config/suivi/config.toml`:

```toml
[tracking]
human_buffer_secs = 300  # time budgeted for reading/writing prompts
retention_days = 365

[[projects]]
path = "~/code/my-project"

[[projects]]
path = "~/code/org/*"  # tracks each subdirectory individually
```

## Supported Agents

| Agent | Status |
|-------|--------|
| Claude Code | ✅ v1 |
| Codex | ✅ v1 |
| Pi | ✅ v1 |
| OpenCode | ✅ v1 (session-level granularity) |

## Contributing

Adding a new agent takes ~30 lines. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
