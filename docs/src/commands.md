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
suivi stats --history          # recent turns (today by default)
suivi stats --projects         # cross-project comparison table
suivi stats --project <name>   # drill into one project
suivi stats --agent <name>     # drill into one agent
suivi stats --all --format json  # full JSON export
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

## suivi hook pre / stop

Internal commands called by agent hooks. Not intended for direct use.

```
suivi hook pre
suivi hook stop
```

Both read JSON from stdin and exit 0 always (errors are silent).
