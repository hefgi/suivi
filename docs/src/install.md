# Install

## Homebrew

```bash
brew install hefgi/tap/suivi
```

## Cargo

```bash
cargo install suivi
```

Requires Rust 1.85+.

## Pre-built binaries

Each release ships static binaries for macOS (Intel and Apple Silicon) and
Linux (x86_64 and aarch64, musl — no glibc dependency):

```bash
# Example: Apple Silicon
curl -sSL https://github.com/hefgi/suivi/releases/download/v0.2.0/suivi-v0.2.0-aarch64-apple-darwin.tar.gz \
  | tar -xz && mv suivi /usr/local/bin/
```

Browse all assets on the [releases page](https://github.com/hefgi/suivi/releases).

## Setup

After installing, run:

```bash
suivi init
```

This creates your config at `~/.config/suivi/config.toml`, detects installed agents, and registers hooks automatically.

## PATH requirements

suivi must be on the PATH that your shell and agent hooks use. On macOS, agent hooks run via `/bin/sh` which does not source `~/.zshrc` or `~/.bashrc`.

**Homebrew install** handles this automatically — the binary lands in `/opt/homebrew/bin` which is always on the default PATH.

**`cargo install`** places the binary in `~/.cargo/bin`. You must ensure this is on your PATH:

```bash
# Add to ~/.zshrc or ~/.bash_profile
export PATH="$HOME/.cargo/bin:$PATH"
```

Then symlink to a system-wide location so hooks work:

```bash
ln -sf ~/.cargo/bin/suivi /opt/homebrew/bin/suivi   # macOS (Apple Silicon)
# or
sudo ln -sf ~/.cargo/bin/suivi /usr/local/bin/suivi  # Linux / macOS Intel
```
