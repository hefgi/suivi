# Commands

## suivi init

Initialize suivi: create the config file and install agent hooks.

```
suivi init
```

Interactive wizard that prompts for project paths to track, then installs hooks for all detected agents.

If the config already exists, the wizard exits early and directs you to `suivi doctor`.

## suivi status

Show hook installation health and recent activity.

```
suivi status
```

Output:

```
Hooks
  Claude Code          Ok
  Codex                Missing
  OpenCode             Ok
  Pi (experimental)    Ok  (experimental)

Recent activity
  12 turns recorded in the last 7 days.
    claude-code          10 turns
    codex                2 turns
```

## suivi stats

Show time analytics.

```
suivi stats [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `--all` | Show all-time stats |
| `--project <path>` | Filter by project path |
| `--agent <name>` | Filter by agent |
| `--projects` | Per-project breakdown |
| `--history` | Turn history |
| `--graph` | ASCII activity graph |
| `--daily` | Daily breakdown |
| `--format text\|json\|csv` | Output format (default: text) |

```bash
suivi stats                    # summary
suivi stats --graph            # daily graph — last 30 days
suivi stats --daily            # day-by-day breakdown
suivi stats --history          # recent turns (last 30 days by default)
suivi stats --projects         # cross-project comparison table
suivi stats --project <path>   # drill into one project
suivi stats --agent <name>     # drill into one agent
suivi stats --all --format json  # full JSON export
```

### --projects output

```
Projects

Project                     Wall-clock   Agent time   Sessions
--------------------------------------------------------------------
tracker-code                     1h 12m       2h 05m  ×4
my-app                             45m          58m   ×1
(untracked)                        12m          18m   ×2
```

### --graph output

```
Activity graph

  2026-05-28  ████████████░░░░░░░░░░░░░░░░░░  42m
  2026-05-29  ██████████████████████░░░░░░░░  1h 12m
  2026-05-30  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  < 1m
  2026-06-01  ██████░░░░░░░░░░░░░░░░░░░░░░░░  18m
  2026-06-02  ████████████████████████████░░  1h 32m
```

### --daily output

```
Daily breakdown

Date          Wall-clock   Agent time    Turns
--------------------------------------------------
2026-05-28         42m           58m       8
2026-05-29       1h 12m        2h 05m     14
2026-06-02       1h 32m        2h 48m     21
```

### --history output

```
History

  2026-06-02 09:14  claude-code           12m   tracker-code          claude-opus-4-5
  2026-06-02 10:31  claude-code            8m   tracker-code          claude-opus-4-5
  2026-06-02 14:05  codex                  5m   my-app                o4-mini
```

### --format json

```bash
suivi stats --all --format json
```

```json
[
  {
    "window": "All time",
    "turns": 42,
    "wall_clock_secs": 7320.0,
    "agent_secs": 14400.0
  }
]
```

### --format csv

```bash
suivi stats --all --format csv
```

```
window,turns,wall_clock_secs,agent_secs
All time,42,7320,14400
```

## suivi doctor

Database maintenance.

```
suivi doctor [--prune] [--check]
```

| Flag | Description |
|------|-------------|
| `--prune` | Delete stale turns (open > 2h) and turns beyond retention period |
| `--check` | Run SQLite PRAGMA integrity_check |

Without flags, shows a summary of stale and beyond-retention turn counts.

## suivi uninstall

Remove suivi's hooks from all agent configs — the inverse of `suivi init`.

```
suivi uninstall [--purge]
```

- Strips `suivi hook` entries from Claude Code's `settings.json` and Codex's
  `hooks.json`, preserving any other hooks; deletes the Pi/OpenCode plugin
  files (only if they actually reference suivi).
- Without `--purge`, your config, database, and logs are left in place and
  their paths are printed.
- `--purge` also deletes suivi's config, database, and log directories.

Run this before deleting the binary — otherwise every agent prompt keeps
invoking a `suivi` command that no longer exists.

## suivi hook pre / stop

Internal commands called by agent hooks. Not intended for direct use.

```
suivi hook pre --agent <id>
suivi hook stop --agent <id>
```

Both read JSON from stdin and exit 0 always (errors are silent).
