# Configuration

Config lives at `~/.config/suivi/config.toml`, created by `suivi init`.

```toml
[tracking]
human_buffer_secs = 300  # time budgeted for reading/writing prompts (default: 5min)
retention_days = 365     # turns older than this are pruned automatically

[[projects]]
path = "~/code/my-project"

[[projects]]
path = "~/code/org/*"    # tracks each subdirectory individually

[[projects]]
path = "~/code/other"
name = "other"           # optional name override (defaults to directory name)
```

## Project matching

On each turn, the agent's working directory is matched against your tracked paths using **nearest-ancestor matching** — the deepest tracked path that is a prefix of the CWD wins.

Glob paths with `*` are expanded at startup. Each matched subdirectory becomes an independent tracked project.
