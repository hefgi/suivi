# Changelog

All notable changes to suivi will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Foundation: error types, agent trait, database schema, config loading, logging utilities
- Agent support: Claude Code, Codex, OpenCode (session-level), Pi (experimental)
- Hook handlers: `suivi hook pre` and `suivi hook stop` with buffer correction
- CLI: `suivi init`, `suivi status`, `suivi doctor --prune --check`
- Analytics: `suivi stats` with today/week/all-time summary, `--projects`, `--history`, `--graph`, `--daily`
- Output formats: `--format text|json|csv` for `suivi stats`
- `CONTRIBUTING.md` at the repo root (the README already linked it) (#21)
- `suivi uninstall` (+ `--purge`): removes suivi's hook entries from every
  agent config, the inverse of `suivi init` (#23)

### Changed
- The second headline metric is now **agent time** — the sum of each turn's
  real duration — replacing "accumulated" effective time, which blended human
  buffers into agent effort and double-counted attention across parallel
  sessions (#10). Summary JSON/CSV rename `accumulated_secs` to `agent_secs`;
  history exports gain an `agent_duration_secs` column. Per-turn
  `effective_duration_secs` is still stored and exported.

### Fixed
- Agent thinking time was always recorded as 0: `hook stop` read a `duration_ms`
  field that no agent sends; durations now fall back to `ended_at − started_at` (#16)
- Turns interrupted before `Stop` fired were lost as stale rows; `hook pre` now
  closes any open turn in the session at the next prompt (#16)
- Agent attribution no longer depends on env/parent-process sniffing: hook
  commands pass `--agent <id>`, the payload `agent` field is honored, and
  sniffing remains only as a fallback for old installs (#17)
- `suivi init` only installs hooks for agents actually present on the machine,
  and re-runs upgrade outdated hook commands in place (#17, #18)
- `cargo test` no longer writes hook files into the developer's real home
  directory (#18)
- SQLite uses WAL and a 5s busy timeout so concurrent hooks from parallel
  sessions don't silently drop turns on lock contention (#19)
- "Today", daily buckets, and `--history` timestamps use the local timezone
  instead of UTC; `--since` dates are local midnight (#20)
- Invalid `--since` dates now error instead of being silently ignored (#20)
- `suivi status` reports "not installed" for absent agents instead of a
  misleading hook-health state (#18)
- Every command panicked with "Broken pipe" when piped to an early-exiting
  consumer; SIGPIPE now uses the default disposition (#23)
- OpenCode tracking recorded one truncated turn per session; the plugin now
  tracks per user message with verified event payloads (#24)
- Both JS plugins shelled out synchronously through temp files on every
  event; payloads now go to suivi's stdin via non-blocking spawn (#24, #25)
- The per-project and per-agent mini-graphs bucketed days in UTC, missing
  the local-timezone fix that covered the other views (#26)
