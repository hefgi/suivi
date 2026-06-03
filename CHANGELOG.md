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
