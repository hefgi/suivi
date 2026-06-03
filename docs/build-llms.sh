#!/usr/bin/env bash
# Generates llms.txt (index) and llms-full.txt (concatenated content)
# into docs/book/ after mdbook build has run.
set -euo pipefail

BOOK_DIR="$(dirname "$0")/book"
SRC_DIR="$(dirname "$0")/src"

cat > "$BOOK_DIR/llms.txt" <<'EOF'
# suivi

> Track time spent working with AI coding agents across multiple projects.

## Getting started

- [Introduction](https://hefgi.github.io/suivi/introduction.html): What suivi is and how it works
- [Install](https://hefgi.github.io/suivi/install.html): Homebrew and cargo
- [Quick start](https://hefgi.github.io/suivi/quickstart.html): Up and running in 2 commands
- [Configuration](https://hefgi.github.io/suivi/configuration.html): config.toml reference

## Reference

- [Commands](https://hefgi.github.io/suivi/commands.html): All CLI flags and options
- [Agents](https://hefgi.github.io/suivi/agents.html): Supported agents and how to add new ones
- [How time is measured](https://hefgi.github.io/suivi/time.html): Wall-clock vs accumulated, buffer cap logic

## Development

- [Contributing](https://hefgi.github.io/suivi/contributing.html): Dev workflow, adding agents, PRs
EOF

OUTPUT="$BOOK_DIR/llms-full.txt"
> "$OUTPUT"

pages=(
  introduction
  install
  quickstart
  configuration
  commands
  agents
  time
  contributing
)

for page in "${pages[@]}"; do
  file="$SRC_DIR/${page}.md"
  if [[ -f "$file" ]]; then
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    cat "$file" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
  fi
done

echo "llms.txt and llms-full.txt written to $BOOK_DIR"
