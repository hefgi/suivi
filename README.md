<div align="center">

<img src="banner.png" alt="suivi" width="600" />

**Track time spent working with AI coding agents across multiple projects.**

Hook into Claude Code, Codex, Pi, OpenCode and more — measure where your time actually goes, broken down by project, agent, and model.

[![CI](https://github.com/hefgi/suivi/actions/workflows/ci.yml/badge.svg)](https://github.com/hefgi/suivi/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/suivi.svg)](https://crates.io/crates/suivi)
[![Homebrew](https://img.shields.io/badge/homebrew-hefgi%2Ftap-orange)](https://github.com/hefgi/homebrew-tap)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-hefgi.github.io%2Fsuivi-blue)](https://hefgi.github.io/suivi/)

---

**Built for developers running multiple AI agent sessions in parallel.**

![Claude Code](https://img.shields.io/badge/Claude_Code-d97706?style=flat-square)
![Codex](https://img.shields.io/badge/Codex-10a37f?style=flat-square)
![Pi](https://img.shields.io/badge/Pi-333?style=flat-square)
![OpenCode](https://img.shields.io/badge/OpenCode-6366f1?style=flat-square)

and any agent that supports hooks or extensions.

</div>

## The problem

You're running 3 Claude Code sessions across different projects, a Codex session on another, and a Pi session reviewing a PR. At the end of the day you have no idea where your time went. Was it accounting? The new API? That refactor you kept context-switching into?

suivi hooks into every agent session automatically. Each prompt you send is a turn. Each turn is attributed to a project. At the end of the day — or week, or month — you see exactly where your time went, split by project, agent, and model.

> suivi is French for "tracking" — what you do when you follow something closely over time.

## Install

[![Homebrew](https://img.shields.io/badge/Homebrew-FBB040?style=flat-square&logo=homebrew&logoColor=black)](https://github.com/hefgi/homebrew-tap)

```bash
brew install hefgi/tap/suivi
```

[![Crates.io](https://img.shields.io/badge/cargo-install-orange?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/suivi)

```bash
cargo install suivi
```

## Setup

```bash
suivi init
```

Creates `~/.config/suivi/config.toml`, detects installed agents, and registers hooks automatically.

## Usage

```bash
suivi stats                    # today, this week, all time
suivi stats --graph            # daily graph — last 30 days
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
human_buffer_secs = 300  # time budgeted for reading/writing prompts (default: 5min)
retention_days = 365

[[projects]]
path = "~/code/my-project"

[[projects]]
path = "~/code/org/*"  # tracks each subdirectory individually
```

Project names default to the directory name. Add `name = "..."` to override.

## Wall-clock vs accumulated time

suivi tracks two time metrics:

- **Wall-clock** — real elapsed time the project occupied your day. Two parallel sessions for 1 min = 1 min.
- **Accumulated** — total agent-hours invested. Two parallel sessions for 1 min = 2 min.

Both are always shown. Wall-clock tells you how your day was spent. Accumulated tells you how much effort went in.

## Supported agents

| Agent | Status | Hook event |
|-------|--------|------------|
| Claude Code | ✅ v1 | `UserPromptSubmit` + `Stop` |
| Codex | ✅ v1 | `UserPromptSubmit` + `Stop` |
| Pi | ✅ v1 | `before_agent_start` + `agent_end` |
| OpenCode | ✅ v1 | `session.created` + `session.idle` (session-level) |

## Contributing

Adding a new agent takes ~30 lines. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
