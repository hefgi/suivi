# Configuration

suivi reads its configuration from `~/.config/suivi/config.toml`
(`$XDG_CONFIG_HOME/suivi/config.toml` if set).

## Example

```toml
[tracking]
human_buffer_secs = 300  # seconds budgeted for reading/writing prompts (default: 300)
retention_days = 365     # how long to keep turn history (default: 365)

[[projects]]
path = "~/code/myapp"
name = "My App"          # optional; defaults to the directory basename

[[projects]]
path = "~/work/client-*" # globs expand to one project per matched directory
```

## Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tracking.human_buffer_secs` | integer | `300` | Seconds of human time budgeted before and after each AI turn (writing the prompt, reading the response). Feeds the wall-clock padding and the effective-duration estimate |
| `tracking.retention_days` | integer | `365` | Days to keep turn history before pruning |

## Projects

Each `[[projects]]` entry tracks one path:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | string | yes | Exact path or glob pattern. Supports `~` and `*` |
| `name` | string | no | Display name shown in `suivi stats`. Defaults to the directory basename |

### Path matching

suivi matches a turn to a project by finding the deepest tracked path that is
an ancestor of the turn's working directory. If two projects both match, the
more specific (longer) path wins.

Globs are re-expanded on every hook call — new directories matching a pattern
are picked up automatically. Only directories count; stray files matching a
glob are ignored.

### Legacy format

Configs written by early versions (`buffer_mins`, `[[projects]] paths = [...]`)
are still read transparently, but new configs should use the format above.

### Creating the config

Run `suivi init` to create the config interactively.
