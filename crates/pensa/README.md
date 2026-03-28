# Pensa — Agent Persistent Memory

Pensa is a Rust CLI (`pn`) that serves as the agent's persistent structured memory. It replaces markdown-based issue logging with a single command interface backed by SQLite. A single command like `pn create "login crash" -p p0 -t bug` replaces the error-prone multi-step process of creating directories and writing markdown files.

Inspired by [beads](https://github.com/steveyegge/beads), rebuilt in Rust with tighter integration into the Springfield workflow.

## Architecture

Pensa uses a **client/daemon** model:

- **Daemon** (`pn daemon`) — runs on the host, owns the SQLite database, handles all reads and writes. Listens on a per-project derived port by default.
- **CLI client** (`pn <command>`) — thin HTTP client that translates subcommands into REST requests to the daemon.

The daemon keeps SQLite behind a single process, making concurrent access from multiple agent sessions safe.

### Storage

```
.pensa/
├── db.sqlite        (working database, gitignored)
├── issues.jsonl     (git-committed export)
├── deps.jsonl       (git-committed export)
├── comments.jsonl   (git-committed export)
├── src_refs.jsonl   (git-committed export)
└── doc_refs.jsonl   (git-committed export)
```

- **SQLite** is the runtime store. Rebuilt from JSONL on clone.
- **JSONL** files are snapshots created by `pn export`, committed to git. Human-readable, diff-friendly.

## Quick Start

```bash
# Start the daemon (foreground, per-project derived port)
pn daemon

# In another terminal — create an issue
pn create "login crash on empty password" -t bug -p p0

# List all open issues
pn list

# Claim an issue for work
pn update <id> --claim

# Close when done
pn close <id> --reason "fixed"

# Check project health
pn status
```

## Command Reference

See the `pensa` forma spec (`fm show pensa`) for the full specification. Summary:

### Issues
```
pn create "title" -t <type> [-p <pri>] [-a <assignee>] [--spec <stem>] [--fixes <bug-id>] [--dep <id>...]
pn show <id>
pn update <id> [--title <t>] [--priority <p>] [--claim] [--unclaim] ...
pn close <id> [--reason "..."] [--force]
pn reopen <id> [--reason "..."]
pn release <id>
pn delete <id> [--force]
```

### Queries
```
pn list [--status <s>] [--priority <p>] [-t <type>] [-n <limit>] ...
pn ready [-n <limit>] [-p <pri>] ...
pn blocked
pn search <query>
pn count [--by-status] [--by-priority] [--by-issue-type] [--by-assignee]
pn status
pn history <id>
```

### Dependencies
```
pn dep add <child> <parent>
pn dep remove <child> <parent>
pn dep list <id>
pn dep tree <id> [--direction up|down]
pn dep cycles
```

### Comments
```
pn comment add <id> "text"
pn comment list <id>
```

### Source References
```
pn src-ref add <id> <path> [--reason "..."]
pn src-ref list <id>
pn src-ref remove <ref-id>
```

### Documentation References
```
pn doc-ref add <id> <path> [--reason "..."]
pn doc-ref list <id>
pn doc-ref remove <ref-id>
```

### Data & Maintenance
```
pn export          # SQLite → JSONL, then git add
pn import          # JSONL → SQLite
pn doctor [--fix]  # Health checks + optional auto-fix
pn where           # Print .pensa/ path
```

### Daemon
```
pn daemon [--port <port>] [--project-dir <path>]
pn daemon status
```

#### Shutdown Conditions

The daemon shuts down on any of:

1. **Ctrl+C** (`tokio::signal::ctrl_c()`)
2. **SIGTERM** (`tokio::signal::unix::signal(SignalKind::terminate())`)
3. **`/shutdown` endpoint** (internal — used by test fixtures, not the CLI)
4. **Project directory watchdog** (3 consecutive failures at 5s interval)

#### Project Directory Watchdog

The daemon monitors the existence of its `--project-dir` on a fixed 5-second interval. If the directory does not exist for 3 consecutive checks (15 seconds total), the daemon shuts down gracefully. A single successful check resets the failure counter to zero. This prevents the daemon from running indefinitely after the project directory is deleted (e.g., temp dirs in tests, renamed projects).

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `PN_DAEMON` | (port file `.pensa/daemon.port`, then SHA-256 derived port) | Full daemon URL. Overrides all other discovery. |
| `PN_DAEMON_HOST` | `localhost` | Daemon hostname. Combined with port from port file or SHA-256 derivation. |
| `PN_ACTOR` | (git user / $USER) | Actor name for audit trail. Overridden by `--actor` flag. |

## Testing

```bash
# Run all pensa tests (35 unit tests)
cargo test -p pensa

# Run a specific test
cargo test -p pensa <test_name>
```

Tests use an in-memory SQLite database via `tempfile` for isolation. Each test gets a fresh database.
