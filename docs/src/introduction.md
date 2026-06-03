# Introduction

suivi tracks time spent working with AI coding agents across multiple projects.

It hooks into Claude Code, Codex, Pi, OpenCode, and other CLI-based agents to measure how much time you spend on each project — broken down by agent, model, and day. Shows both **wall-clock time** (real elapsed) and **accumulated time** (total agent-hours invested).

## How it works

suivi registers hooks in your agent's config. When you submit a prompt, `suivi hook pre` fires. When the agent finishes responding, `suivi hook stop` fires. The time between them — plus a configurable human buffer — is attributed to the project matching your current working directory.

Run multiple sessions in parallel and suivi handles it correctly: wall-clock time deduplicates overlapping sessions, accumulated time adds them up.
