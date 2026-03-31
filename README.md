<div align="center">

# lnch

**One YAML. One command. All your services.**

[![Crates.io](https://img.shields.io/crates/v/lnch)](https://crates.io/crates/lnch)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](https://github.com/shell-term/lnch/releases)

A TUI multi-process launcher for your dev environment — manage all your local servers and commands from a single terminal.

![demo](assets/lnch.gif)

</div>

**English** | [日本語](README.ja.md)

## Features

- **One YAML, one command** — Define all your services in `lnch.yaml` and launch them with `lnch`
- **TUI dashboard** — Monitor process status and logs in a split-pane terminal UI powered by [ratatui](https://github.com/ratatui/ratatui)
- **Dependency ordering** — `depends_on` ensures services start in the right order via topological sort, with readiness checks (`ready_check`) to wait until dependencies are actually ready
- **Auto-discovery** — `lnch` searches up the directory tree for `lnch.yaml`, so it works from any subdirectory
- **Per-task logs** — stdout/stderr captured in ring buffers with color-coded display
- **Graceful shutdown** — SIGTERM with timeout, then SIGKILL; process groups ensure no orphan processes
- **Cross-platform** — macOS, Linux (including WSL), and Windows
- **Update notifications** — Automatically checks for new releases on startup; press `u` to install the update directly from the TUI

## Installation

### macOS / Linux

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.sh | sh
```

### Windows (PowerShell)

```powershell
powershell -c "irm https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.ps1 | iex"
```

### cargo-binstall

```bash
cargo binstall lnch
```

### From source (requires [Rust toolchain](https://rustup.rs/))

```bash
cargo install --path .
```

### Build locally

```bash
git clone https://github.com/shell-term/lnch.git
cd lnch
cargo build --release
# Binary is at ./target/release/lnch
```

## Quick Start

**1. Create a `lnch.yaml` in your project root:**

```yaml
name: my-project

tasks:
  - name: frontend
    command: npm run dev
    working_dir: ./frontend
    env:
      PORT: "3000"
    color: green

  - name: backend
    command: cargo run -- --port 8080
    working_dir: ./backend
    color: blue
    depends_on:
      - database

  - name: database
    command: docker compose up postgres
    color: magenta
```

**2. Launch:**

```bash
lnch
```

That's it. All three services start in dependency order, and you get a TUI to monitor and control them.

## Usage

```
lnch                     # Auto-detect lnch.yaml and launch TUI
lnch --config <path>     # Use a specific config file
lnch --version           # Show version
lnch --help              # Show help
```

### Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Select previous task |
| `↓` / `j` | Select next task |
| `a` | Start all tasks |
| `s` | Start/Stop selected task |
| `r` | Restart selected task |
| `PageUp` | Scroll logs up |
| `PageDown` | Scroll logs down |
| `Home` | Scroll to top of logs |
| `End` | Scroll to bottom (resume auto-scroll) |
| `c` | Clear logs of selected task |
| `u` | Update lnch (shown when a new version is available) |
| `q` / `Ctrl+C` | Quit (graceful shutdown) |

## Configuration

### `lnch.yaml` Schema

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | `string` | Yes | — | Project name (shown in TUI title bar) |
| `tasks` | `list` | Yes | — | List of task definitions (at least one) |
| `tasks[].name` | `string` | Yes | — | Task name (must be unique) |
| `tasks[].command` | `string` | Yes | — | Shell command to execute |
| `tasks[].working_dir` | `string` | No | Config file directory | Working directory (relative to `lnch.yaml` or absolute) |
| `tasks[].env` | `map` | No | `{}` | Environment variables (inherits parent env, then overrides) |
| `tasks[].color` | `string` | No | Auto-assigned | Task color: `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` |
| `tasks[].depends_on` | `list` | No | `[]` | Tasks that must start before this one |
| `tasks[].ready_check` | `object` | No | Smart default | Readiness check for dependency waiting (see below) |

### Config File Discovery

When run without `--config`, `lnch` searches for `lnch.yaml` starting from the current directory and walking up the tree (up to 10 levels). This means you can run `lnch` from any subdirectory of your project.

### Ready Checks

When a task has `depends_on`, lnch waits for each dependency to become "ready" before starting the dependent task. By default, smart detection is used (one-shot tasks are ready when they exit successfully; long-running tasks are ready after a 2-second grace period). For more control, use `ready_check`:

```yaml
tasks:
  - name: database
    command: docker compose up postgres
    ready_check:
      tcp: { port: 5432 }     # Wait for TCP port to accept connections
      timeout: 60              # Timeout in seconds (default: 30)

  - name: migrate
    command: sqlx migrate run
    ready_check:
      exit: {}                 # Wait for the process to exit with code 0

  - name: backend
    command: cargo run
    depends_on: [database, migrate]
    ready_check:
      log_line: { pattern: "listening on port" }  # Wait for log output pattern

  - name: frontend
    command: npm run dev
    depends_on: [backend]
    # No ready_check: smart default (grace period for long-running process)
```

| Check type | Description |
|-----------|-------------|
| `tcp: { port: <N> }` | Wait for a TCP connection to succeed on `127.0.0.1:<port>` |
| `http: { url: "<URL>", status: <N> }` | Wait for an HTTP GET to return the expected status (default: 200). HTTP only, not HTTPS. |
| `log_line: { pattern: "<text>" }` | Wait for a substring match in stdout/stderr |
| `exit: {}` | Wait for the process to exit with code 0 |

Common options: `timeout` (seconds, default: 30), `interval` (milliseconds, default: 500).

If the readiness check times out, lnch logs a warning and continues starting the next group.

### Command Execution

Commands are executed through the system shell:

| OS | Shell |
|----|-------|
| macOS / Linux | `sh -c "<command>"` |
| Windows | `cmd /C "<command>"` |

This allows pipes, redirects, and variable expansion to work as expected.

## Architecture

```
lnch
├── CLI (clap)          — argument parsing, config path resolution
├── Config              — YAML loading, validation, dependency resolution
├── Process Manager     — async task orchestration via tokio channels
│   └── Task Runners    — individual process lifecycle (spawn, IO, signals)
└── TUI (ratatui)       — split-pane UI with event loop
```

Components communicate via `tokio::mpsc` channels:
- **App → ProcessManager**: `ProcessCommand` (Start, Stop, Restart, Shutdown)
- **ProcessManager → App**: `ProcessEvent` (StatusChanged, LogLine, ProcessExited)

See [`docs/`](docs/) for detailed design documents (in Japanese).

## Comparison with mprocs

| Feature | mprocs | lnch |
|---------|--------|------|
| TUI process management | ✅ | ✅ |
| YAML config | ✅ | ✅ |
| Auto-discovery (walk up directory tree) | ❌ | ✅ |
| `depends_on` startup ordering | ❌ | ✅ |
| Global profiles (multi-project) | ❌ | ✅ (v0.2) |
| TUI config editing | ❌ | ✅ (v0.3) |
| `init` command with templates | ❌ | ✅ (v0.4) |
| Health checks & auto-restart | ❌ | ✅ (v0.5) |

## Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| **v0.1** | MVP — YAML config, TUI, process management, `depends_on` | 🚧 In Progress |
| **v0.2** | Global profiles (`~/.config/lnch/profiles.yaml`) | Planned |
| **v0.3** | TUI-based config editing | Planned |
| **v0.4** | `lnch init` with interactive prompts & templates | Planned |
| **v0.5** | Health checks & auto-restart policies | Planned |

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

```bash
# Run the project
cargo run

# Run with a specific config
cargo run -- --config path/to/lnch.yaml

# Run tests
cargo test

# Check formatting & lints
cargo fmt --check
cargo clippy
```

## License

[MIT](LICENSE) © 2026 shell-term
