# forma (`fm`)

Specification management — a Rust CLI backed by SQLite that replaces manual markdown-based spec tracking with a single command interface.

## What it does

`fm` manages structured specifications for any repository. Specs are the source of truth for all code. All spec mutations go through `fm` — agents and humans read the generated markdown, but never edit it directly.

A single command like `fm create auth --src crates/auth/ --purpose "Authentication"` replaces the error-prone process of creating markdown files and updating index tables by hand.

## Installation

```
cargo install --path crates/forma
```

This installs the `fm` binary.

## Usage

```
fm create <stem> [--src <path>] --purpose "<text>"    # Create a spec
fm show <stem> --json                                  # Show spec details
fm list [--status <status>] --json                     # List specs
fm update <stem> [--status <s>] [--purpose "<text>"]   # Update metadata
fm section set <stem> "<slug>" --body-stdin             # Update a section body
fm export                                              # Generate markdown artifacts
fm check --json                                        # Validate specs
```

## Architecture

```
crates/forma/
├── src/
│   ├── main.rs       # CLI entry, clap commands, HTTP client, daemon auto-start
│   ├── lib.rs        # Library root, re-exports modules
│   ├── client.rs     # HTTP client for the forma daemon
│   ├── db.rs         # SQLite schema, migrations, database operations
│   ├── daemon.rs     # Axum HTTP server, route handlers
│   ├── types.rs      # Spec, Section, Ref, Event, Status types
│   └── output.rs     # Human-readable formatting (non-JSON output)
├── tests/
│   ├── integration.rs  # CLI-level integration tests
│   └── cli_client.rs   # Client API integration tests
└── Cargo.toml
```

## Full documentation

See the spec for complete documentation: `.forma/specs/forma.md`
