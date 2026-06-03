# Configuration

suivi reads its configuration from `~/.config/suivi/config.toml`.

## Example

```toml
buffer_mins = 5      # minutes of human time before/after each turn (default: 5)
retention_days = 90  # how long to keep turn history (default: 90)

[[projects]]
paths = ["~/code/myapp", "~/work/client-*"]
name = "My App"

[[projects]]
paths = ["/home/user/oss/**"]
```

## Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `buffer_mins` | integer | `5` | Minutes of human time budgeted before and after each AI turn (writing the prompt, reading the response) |
| `retention_days` | integer | `90` | Number of days to keep turn history before pruning |

## Projects

Each `[[projects]]` entry maps one or more path patterns to a project:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `paths` | array of strings | yes | Glob patterns or exact paths. Supports `~` and `*` wildcards |
| `name` | string | no | Display name shown in `suivi stats`. Defaults to the directory basename |

### Path matching

suivi matches a turn to a project by finding the nearest ancestor of the current working directory that appears in any project's `paths`. If two projects both match, the more specific (longer) path wins.

Globs are re-expanded on every hook call — new directories matching a pattern are picked up automatically.

### Creating the config

Run `suivi init` to create the config interactively.
