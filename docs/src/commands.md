# Commands

## suivi stats

Summary view: today, this week, all time. Shows wall-clock and accumulated time, top projects, top agents.

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

## Time flags

| Flag | Window |
|------|--------|
| *(default)* | today + last 7 days + all time |
| `--today` | today only |
| `--week` | last 7 rolling days |
| `--month` | last 30 rolling days |
| `--all` | all time |
| `--since <date>` | from YYYY-MM-DD |

## Output flags

| Flag | Effect |
|------|--------|
| `--format json` | JSON output |
| `--format csv` | CSV output |

## suivi init

First-time setup and hook re-sync.

```bash
suivi init
```

## suivi status

Hook health, config path, database path, untracked activity.

```bash
suivi status
```
