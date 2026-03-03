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
