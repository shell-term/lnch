# v0.1.4

## Bug Fixes

- **Fix log display truncation for rapid output** -- The event loop processed only one event per draw cycle, causing the log view to lag behind fast-producing processes and the tail to appear cut off. Now drains all pending events before each redraw.

# v0.1.3

## New Features

- **Windows ConPTY support** -- Use Windows Pseudo Console (ConPTY) for child processes so that grandchild processes (e.g. Python multiprocessing workers) receive valid console handles. Fixes `OSError: [Errno 22] Invalid argument` on Windows when launched programs use multiprocessing. Falls back to pipe mode on older Windows versions. Unix behavior is unchanged.

# v0.1.2

## Bug Fixes

- **Fix Python stdout not appearing in TUI** -- Python's stdout defaults to block buffering when piped, preventing log messages (e.g. "Frontend serving at ...") from reaching the TUI. Now sets `PYTHONUNBUFFERED=1` for all spawned processes.

# v0.1.1

## Bug Fixes

- **Fix log scroll not reaching bottom** -- Log view scroll could not reach the end of output when long lines (e.g. error messages) triggered word-wrapping. Replaced approximate visual line counting with ratatui's exact `Paragraph::line_count()`.

## Maintenance

- **Update all dependencies** -- ratatui 0.29→0.30, crossterm 0.28→0.29, nix 0.29→0.31, windows-sys 0.59→0.61
- **Code quality** -- Apply rustfmt across all source files; fix clippy warnings (unused imports, idiomatic API usage)

# v0.1.0

Initial release of lnch - a TUI multi-process launcher for your dev environment.

## Highlights

- **One YAML, one command** -- Define all your services in `lnch.yaml` and launch them with `lnch`
- **TUI dashboard** -- Monitor process status and logs in a split-pane terminal UI powered by ratatui
- **Dependency ordering** -- `depends_on` ensures services start in the right order via topological sort
- **Auto-discovery** -- Searches up the directory tree for `lnch.yaml`, so it works from any subdirectory
- **Per-task logs** -- stdout/stderr captured in ring buffers with color-coded display
- **Graceful shutdown** -- SIGTERM with timeout, then SIGKILL; process groups ensure no orphan processes
- **Cross-platform** -- macOS, Linux (including WSL), and Windows
