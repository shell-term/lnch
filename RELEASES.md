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
