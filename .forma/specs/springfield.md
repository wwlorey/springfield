# springfield Specification

CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | draft |

## Overview

CLI entry point for Springfield. All developer interaction goes through this binary. It handles project scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle. Delegates iteration execution to ralph and persistent memory to pensa.

`sgf` provides:
- **Project scaffolding**: `sgf init` creates the project structure (`.sgf/`, `.pensa/`, specs index, Claude deny settings, git hooks)
- **Unified command dispatch**: `sgf <command>` resolves to a prompt file using layered `.sgf/` lookup (local `./.sgf/prompts/` → global `~/.sgf/prompts/`), with per-command defaults from `config.toml`
- **Prompt delivery**: Validate prompt files exist via layered lookup, pass raw paths to ralph or `cl`, set `SGF_SPEC` env var
- **Loop orchestration**: Launch ralph with the correct flags, manage PID files, tee logs
- **Recovery**: Pre-launch cleanup of dirty state from crashed iterations
- **Daemon lifecycle**: Start the pensa daemon before launching loops

## Architecture

## Per-Repo Project Structure

After `sgf init` and ongoing development, a project contains:

```
.pensa/
├── db.sqlite                  (gitignored — daemon-owned working database)
├── issues.jsonl               (committed — git-portable export)
├── deps.jsonl                 (committed)
└── comments.jsonl             (committed)
.sgf/
├── MEMENTO.md                 (fm/pn workflow reference — authored per-project)
├── BACKPRESSURE.md            (build/test/lint/format reference — authored per-project)
├── logs/                      (gitignored — AFK loop output)
│   └── <loop-id>.log
├── run/                       (gitignored — PID files and session metadata for running/completed loops)
│   ├── <loop-id>.pid
│   └── <loop-id>.json         (session metadata — session_id, loop config, status; see session-resume spec)
└── prompts/                   (optional — project-local overrides only)
    └── build.md               (example: overrides just build.md, other prompts fall through to ~/.sgf/)
.pre-commit-config.yaml        (prek hooks for pensa sync)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (`ln -s` to AGENTS.md)
test-report.md                 (generated — overwritten each test run, committed)
verification-report.md         (generated — overwritten each verify run, committed)
specs/
├── README.md                  (agent-maintained spec index — loom-style tables)
└── *.md                       (prose specification files)
```

### Global Home Structure

Populated by `just install` (rsync from the springfield repo's `.sgf/`):

```
~/.sgf/
├── MEMENTO.md                 (universal agent instructions — fm/pn workflows, conventions)
├── BACKPRESSURE.md            (universal build/test/lint/format reference)
└── prompts/
    ├── config.toml            (per-command defaults: mode, iterations, auto_push, alias)
    ├── build.md               (default prompts for all projects)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    ├── issues-log.md
    └── doc.md
```

### Installation

All crates are installed via `just install`, which also syncs the global `~/.sgf/` directory:

```just
install:
    cargo install --path crates/pensa
    cargo install --path crates/ralph
    cargo install --path crates/springfield
    cargo install --path crates/claude-wrapper
    rsync -av --delete --exclude='logs/' --exclude='run/' .sgf/ ~/.sgf/
```

The rsync copies prompts, config, MEMENTO.md, and BACKPRESSURE.md to `~/.sgf/`. The `--delete` flag removes files from `~/.sgf/` that no longer exist in the repo. Runtime directories (`logs/`, `run/`) are excluded.

### File Purposes

**`~/.sgf/BACKPRESSURE.md`** — Universal build, test, lint, and format commands. Developer-editable. Override per-project by placing a `BACKPRESSURE.md` in `./.sgf/`. Injected into every Claude session by `cl` (see claude-wrapper spec).

**`~/.sgf/MEMENTO.md`** — Universal agent instructions (fm/pn workflows, conventions, sandbox rules). Override per-project by placing a `MEMENTO.md` in `./.sgf/`. Injected into every Claude session by `cl`.

**`AGENTS.md`** — Hand-authored operational guidance. Contains code style preferences, runtime notes, and special instructions. Created as an empty file by `sgf init`.

**`CLAUDE.md`** — Entry point for Claude Code. Symlinks to AGENTS.md. Auto-loaded by Claude Code at the start of every session.

**`config.toml`** — Per-command defaults. Defines `mode`, `iterations`, `auto_push`, and optional `alias` for each prompt. Lives in `~/.sgf/prompts/` (global) with optional per-project override in `./.sgf/prompts/config.toml`. Local config sections override global ones by key; global sections not overridden locally are preserved. See Prompt Configuration.

**`~/.sgf/prompts/`** — Default prompts for all projects. Synced from the springfield repo via `just install`. To override a prompt for a specific project, create `./.sgf/prompts/<name>.md` — that file takes precedence for that project only. Adding a new `.md` file to either location makes it available as `sgf <name>` immediately (with fallback defaults if no config.toml entry exists).

**`.sgf/run/{loop_id}.json`** — Session metadata file. Contains `session_id` (UUID), loop config (`stage`, `spec`, `mode`, `prompt`, `iterations_completed`, `iterations_total`), and `status` (`running`, `completed`, `interrupted`, `exhausted`). Written before spawning cl/ralph and updated on exit. Enables `sgf resume` to restart previous sessions. See [session-resume spec](session-resume.md) for the full schema.

**`.sgf/` and `.claude/` protection** — Both `.sgf/` and `.claude/` are protected from agent modification via Claude deny settings. `sgf init` scaffolds these rules. `.sgf/` protection prevents agents from modifying local overrides and reference files. `.claude/` protection prevents agents from weakening sandbox configuration or deny rules.

**`SGF_SPEC`** (env var) — Spec stem for build/test stages. Set by sgf in ralph's environment (e.g., `SGF_SPEC=auth`). Ralph includes `./specs/${SGF_SPEC}.md` in its `study` instruction. Prompt files reference this env var directly (e.g., `$SGF_SPEC`).

**`specs/`** — Prose specification files (one per topic of concern). Authored during the spec phase, consumed during builds. Indexed in `specs/README.md`.

## Dependencies

See Cargo.toml. Key dependencies: clap (CLI), tokio (async runtime), toml (config parsing), sha2 (port derivation), vcs-utils (workspace, git operations), shutdown (workspace, signal handling).

## Error Handling

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|--------------|
| `0` | Sentinel found (`.ralph-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

### Recovery Failure Modes

- **Git checkout/clean failure**: Fatal — loop launch is aborted. Proceeding with dirty state would violate the atomic iteration guarantee.
- **`pn doctor --fix` failure**: Warning only — supplementary, not critical for state consistency.
- **Daemon startup failure**: Fatal — loop cannot proceed without pensa/forma daemons. 5-second deadline with exponential backoff.

### Claude Code Crashes and Push Failures

Claude Code crashes and push failures are handled within ralph as warnings — they do not produce distinct exit codes. Ralph logs the failure and continues to the next iteration without cleanup. The next iteration's agent inherits whatever state exists and proceeds via forward correction. Stale claims and dirty working trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery before the next run.

## Testing

Springfield is tested via integration tests that exercise the full CLI. Key scenarios: sgf init idempotence, command resolution with aliases, config.toml layered merge, pre-launch recovery, daemon lifecycle, signal handling (double Ctrl+C/Ctrl+D), loop ID generation, console output formatting.

## CLI Commands

```
sgf <command> [spec] [-a | -i] [-n N] [--no-push]   — run a prompt-driven command
sgf init [--force]                                    — scaffold a new project
sgf logs <loop-id>                                    — tail a running loop's output
sgf resume [loop-id]                                  — resume a previous session
sgf status                                            — show project state (future work)
```

Where `<command>` resolves to a prompt file via layered `.sgf/` lookup. Commands can also be invoked by alias (e.g., `sgf b` for `sgf build` if `alias = "b"` is configured in `config.toml`).

### Command Resolution

1. Check if `<command>` matches a reserved built-in (`init`, `logs`, `resume`, `status`). If so, run the built-in.
2. Check if `./.sgf/prompts/<command>.md` exists (local override). If so, run it.
3. Check if `~/.sgf/prompts/<command>.md` exists (global default). If so, run it.
4. Check if `<command>` matches an alias in the resolved `config.toml` (see Layered Resolution). If so, resolve to the aliased prompt and run it.
5. Error: `unknown command: <command>`.

### Reserved Built-in: `resume`

```
sgf resume [loop_id]
```

Resume a previous Claude session. With `loop_id`: reads session metadata from `.sgf/run/{loop_id}.json`, launches `cl --resume <session_id>` in interactive mode. Without `loop_id`: displays an interactive picker showing recent sessions (newest first, max 20), the user selects one to resume. See [session-resume spec](session-resume.md) for full details.

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | from config.toml | AFK mode: NDJSON stream parsing with formatted output |
| `-i` / `--interactive` | from config.toml | Interactive mode: full terminal passthrough |
| `--no-push` | `false` | Disable auto-push after commits |
| `-n` / `--iterations` | from config.toml | Number of iterations |

`-a` and `-i` are mutually exclusive — passing both is an error (exit 1 with a clear message). When neither is passed, the default comes from `config.toml`. When no `config.toml` entry exists for the command, the fallback default is interactive mode.

### Examples

```bash
sgf build auth -a -n 30        # AFK build loop with spec, 30 iterations
sgf b auth                     # same as sgf build auth (with config.toml defaults)
sgf spec                       # interactive spec session (from config.toml defaults)
sgf build auth -i              # force interactive build (overrides config.toml)
sgf verify -a                  # force AFK verify
sgf issues-log                 # interactive bug reporting
sgf doc                        # interactive doc triage
sgf resume                     # interactive session picker
sgf resume spec-20260316T120000  # resume specific session
```

## Prompt Configuration

`config.toml` defines per-command defaults. Lives in the `.sgf/prompts/` directory and follows the same layered resolution as prompt files (local `./.sgf/prompts/config.toml` → global `~/.sgf/prompts/config.toml`). Parsed at command dispatch time.

### Layered Resolution

All prompt files and `config.toml` use two-tier lookup:

1. `./.sgf/prompts/<file>` — project-local override
2. `~/.sgf/prompts/<file>` — global default

The first existing path wins, on a **file-by-file basis**. A project that overrides only `build.md` locally still uses all other prompts from `~/.sgf/prompts/`.

For `config.toml`, the local file (if present) is merged key-by-key with the global file. Local `[sections]` override global `[sections]` of the same name; global sections not present locally are inherited.

### Format

```toml
[build]
alias = "b"
mode = "interactive"
iterations = 30
auto_push = true

[spec]
alias = "s"
mode = "interactive"
iterations = 1
auto_push = false

[doc]
mode = "interactive"
iterations = 1
auto_push = false

[verify]
alias = "v"
mode = "afk"
iterations = 30
auto_push = true

[test-plan]
mode = "afk"
iterations = 30
auto_push = true

[test]
mode = "afk"
iterations = 30
auto_push = true

[issues-log]
mode = "interactive"
iterations = 1
auto_push = false
```

### Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `alias` | string | — | Short alias for the command (e.g., `"b"` for build). Optional. |
| `mode` | `"afk"` \| `"interactive"` | `"interactive"` | Default execution mode |
| `iterations` | u32 | `1` | Default iteration count |
| `auto_push` | bool | `false` | Auto-push after commits |

### Validation

- Aliases must be unique across all config entries. Duplicate aliases are a parse-time error (exit 1).
- Aliases cannot shadow prompt file names. If `build.md` exists, no other entry can use `alias = "build"`.
- One alias per prompt (the `alias` field is a single string, not an array).

### Fallback Defaults

If a prompt file exists in `.sgf/prompts/` but has no corresponding `[section]` in `config.toml`, sensible defaults apply: `mode = "interactive"`, `iterations = 1`, `auto_push = false`. This allows users to drop in a new prompt file and run it immediately without editing config.toml.

### CLI Override Precedence

CLI flags override config.toml defaults. Config.toml defaults override fallback defaults.

```
CLI flags  >  config.toml  >  fallback defaults
```

## sgf init

## sgf init

Scaffolds a new project. Creates the project-local directory structure and configuration files. Does **not** write prompt files or context files — those live in the global `~/.sgf/` (synced via `just install`). Accepts `--force` to overwrite skeleton files with built-in defaults.

### What it creates

```
.pensa/                                (directory only — daemon creates db.sqlite on start)
.forma/                                (directory only — daemon creates db.sqlite on start)
.sgf/
├── logs/                              (empty, gitignored)
└── run/                               (empty, gitignored)
.claude/settings.json                  (deny rules for .sgf/** and .claude/**)
.pre-commit-config.yaml                (prek hooks for pensa + forma sync)
.gitignore                             (Springfield entries + stack-specific entries)
AGENTS.md                              (empty file)
CLAUDE.md                              (`ln -s` to AGENTS.md)
```

No `.sgf/prompts/` directory is created — prompts and config.toml resolve via layered lookup (local `./.sgf/prompts/` → global `~/.sgf/prompts/`). Users create `./.sgf/prompts/` manually when they need project-local overrides.

Warns if `.sgf/MEMENTO.md` or `.sgf/BACKPRESSURE.md` is missing — agents need these for fm/pn workflow reference and build/test/lint commands. These files are not scaffolded; they are authored per-project.

### Claude settings

`sgf init` creates or updates `.claude/settings.json` with deny rules protecting `.sgf/` and `.claude/` from agent modification, plus native sandbox configuration:

```json
{
  "permissions": {
    "deny": [
      "Edit .sgf/**",
      "Write .sgf/**",
      "Bash rm .sgf/**",
      "Bash mv .sgf/**",
      "Edit .claude/**",
      "Write .claude/**",
      "Bash rm .claude/**",
      "Bash mv .claude/**"
    ]
  },
  "sandbox": {
    "enabled": true,
    "autoAllowBashIfSandboxed": true,
    "network": {
      "allowedDomains": [
        "localhost",
        "github.com",
        "*.githubusercontent.com",
        "crates.io",
        "*.crates.io"
      ],
      "allowLocalBinding": true
    }
  }
}
```

If `.claude/settings.json` already exists, `sgf init` merges both deny rules and sandbox settings into the existing file without duplicating entries or removing existing rules. Array fields (`permissions.deny`, `sandbox.network.allowedDomains`) are merged additively — existing entries are preserved, new entries are appended if not already present. Scalar fields (`sandbox.enabled`, `sandbox.autoAllowBashIfSandboxed`, `sandbox.network.allowLocalBinding`) are set only if not already present in the file.

#### Sandbox configuration

Claude Code's native sandbox provides OS-level filesystem and network isolation using Seatbelt (macOS) and bubblewrap (Linux/WSL2). The scaffolded configuration enables sandbox for all sessions — both interactive and automated.

| Setting | Value | Rationale |
|---------|-------|-----------|
| `sandbox.enabled` | `true` | OS-level enforcement for all sessions |
| `sandbox.autoAllowBashIfSandboxed` | `true` | Bash commands auto-approved within sandbox bounds, reducing prompt fatigue |
| `sandbox.network.allowedDomains` | `["localhost", "github.com", "*.githubusercontent.com", "crates.io", "*.crates.io"]` | `localhost` for pensa daemon access; GitHub for git operations; crates.io for cargo |
| `sandbox.network.allowLocalBinding` | `true` | Allows test servers (e.g., `cargo test`) to bind localhost ports |

**Automated stages (ralph):** Ralph overrides `sandbox.allowUnsandboxedCommands` to `false` via `--settings`, preventing the agent from escaping the sandbox. Combined with `--dangerously-skip-permissions`, this means automated agents operate freely within sandbox bounds but cannot break out.

**Interactive stages:** Use project settings as-is. The sandbox is active; `allowUnsandboxedCommands` is left to the developer's global settings.

**Extending for other stacks:** The default domains cover Rust development. Developers add domains for their stack (e.g., `registry.npmjs.org`, `registry.yarnpkg.com` for Node; `pypi.org` for Python) by editing `.claude/settings.json`. Additional filesystem write paths (e.g., `~/.npm`, `~/.cache`) follow the same pattern via global settings.

### Prek hooks

[prek](https://github.com/j178/prek) is a Rust-based git hook manager that reads `.pre-commit-config.yaml`. It replaces the Python-based [pre-commit](https://pre-commit.com/) — same config format, no Python dependency. `sgf init` generates the config and runs `prek install` to wire the hooks into `.git/hooks/`.

`sgf init` creates `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: pensa export
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: pensa import
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
      - id: forma-export
        name: forma export
        entry: fm export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: forma-import
        name: forma import
        entry: fm import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
```

If `.pre-commit-config.yaml` already exists, `sgf init` appends the pensa and forma hooks without duplicating them.

### Gitignore

`sgf init` creates `.gitignore` or appends entries to an existing one. Entries are added idempotently — existing lines are not duplicated.

#### Entries added

```gitignore
# Springfield
.pensa/db.sqlite
**/.pensa/daemon.port
**/.pensa/daemon.project
**/.pensa/daemon.url
**/.forma/daemon.port
**/.forma/daemon.project
**/.forma/daemon.url
.sgf/logs/
.sgf/run/
.ralph-complete
.ralph-ding

# Rust
/target

# Node
node_modules/

# SvelteKit
.svelte-kit/

# Environment
.env
.env.local
.env.*.local

# macOS
.DS_Store
```

All entries are always added regardless of what exists in the directory. If an entry already exists anywhere in the file, it is not added again.

### CLAUDE.md

`ln -s` to AGENTS.md.

### Idempotence

`sgf init` is safe to re-run. It skips files that already exist (AGENTS.md, CLAUDE.md) and only merges additive content (deny rules, git hooks, gitignore entries). It never overwrites existing content. `prek install` is always run to ensure hooks are wired into `.git/hooks/`.

### --force

`sgf init --force` overwrites skeleton files with built-in defaults, **except `AGENTS.md`** which is never overwritten. Since init no longer scaffolds prompt files or templates, `--force` primarily affects skeleton files.

Safety checks:
- Fails if any target file has uncommitted changes or is untracked by git.
- Lists files to be overwritten and requires `y` confirmation before proceeding.

Config merges (`.gitignore`, `.claude/settings.json`, `.pre-commit-config.yaml`) are unaffected by `--force` — they always use additive merge logic.

## Prompt Delivery

## Prompt Delivery

sgf does not assemble, transform, or preprocess prompt files. Prompts in `.sgf/prompts/` are final — passed directly to ralph or `cl`.

### What sgf Does

1. **Resolve prompt** — find `.sgf/prompts/<stage>.md` via layered lookup (local `./.sgf/prompts/` → global `~/.sgf/prompts/`). Fail with a clear error if not found in either location.
2. **Validate spec** — for stages given a spec (`build [spec]`, `test [spec]`), confirm `specs/<spec>.md` exists. Fail with a clear error (e.g., `spec not found: specs/auth.md`) if the file is missing. Skip this step when no spec is provided.
3. **Set environment** — when a spec is provided, set `SGF_SPEC=<stem>` in ralph's environment. When no spec is given, neither `SGF_SPEC` nor `--spec` are set.
4. **Inject spec study arg** — when a spec is provided, pass `--append-system-prompt 'study @./specs/<stem>.md'` to the invocation (`cl` in interactive mode, ralph via `--spec` in AFK mode). This ensures the agent actively reads the spec in both modes.
5. **Pass the raw path** — give ralph or `cl` the resolved prompt path directly (no intermediate files).

### System Prompt Injection

**Context files (MEMENTO, BACKPRESSURE, specs/README):** `cl` handles injection for all modes — both interactive and automated. `cl` resolves each context file via layered `.sgf/` lookup, builds `--append-system-prompt "study @<file>;..."`, and forwards to the downstream binary. See [claude-wrapper spec](claude-wrapper.md).

**Spec files:** Injected in both modes. In AFK mode, ralph passes `--spec <stem>` study args via its own `--append-system-prompt` to `cl`. In interactive mode, sgf passes `--append-system-prompt 'study @./specs/<stem>.md'` directly to `cl`. All `--append-system-prompt` arguments coexist — `cl`'s context injection and the spec injection are independent.

**sgf does not inject context study args** (MEMENTO, BACKPRESSURE, specs/README — those belong to `cl`). It only injects the spec study arg when calling `cl` directly in interactive mode.

### Prompt Files

Prompts are plain markdown files with no variable substitution. The spec name is available to the agent via:
- The `SGF_SPEC` environment variable (readable by Claude Code)
- The spec file content actively read by the agent via `study` instruction (injected by ralph in AFK mode, by sgf in interactive mode)

For example, `build.md` uses `$SGF_SPEC` directly:
```
Run `pn ready --spec $SGF_SPEC --json`.
```

---

## sgf-to-ralph Contract

## sgf-to-ralph Contract

### Invocation

```
[SGF_SPEC=<stem>] sgf → ralph [-a] [--loop-id ID] [--auto-push BOOL] [--spec STEM] [--session-id UUID] ITERATIONS PROMPT
```

`sgf` translates its own flags and hardcoded defaults into ralph CLI flags. Ralph does not read config files — all configuration arrives via flags and environment variables.

### CLI Flags Passed to Ralph

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `-a` / `--afk` | bool | sgf command (e.g., `sgf build -a`) | AFK mode |
| `--loop-id` | string | sgf-generated | Unique loop identifier |
| `--auto-push` | bool | `true` unless `--no-push` passed to sgf | Auto-push after commits |
| `--spec` | string | spec positional arg from sgf (optional) | Spec stem — ralph includes `./specs/<stem>.md` in its study instruction. Omitted when no spec is given. |
| `--session-id` | string (UUID) | sgf-generated | Pre-assigned session ID for new sessions. sgf generates a UUID before each launch and passes it to ralph. |
| `--resume` | string (UUID) | sgf (from session metadata) | Session ID to resume. Used by `sgf resume` — reads the session ID from `.sgf/run/{loop_id}.json` and passes it to ralph. Mutually exclusive with `--session-id`. |
| `ITERATIONS` | u32 | `-n` / `--iterations` or default `30` | Number of iterations |
| `PROMPT` | path | resolved prompt file path | Raw prompt file (resolved via layered lookup) |

### Environment Variables Passed to Ralph

| Variable | Source | Description |
|----------|--------|-------------|
| `SGF_SPEC` | sgf | Spec stem (e.g., `auth`). Set only when a spec is provided to `build` or `test`. Not set when no spec is given. |

### Execution Model

Execution mode is determined by the resolved `mode` (from CLI flags or config.toml defaults):

| Mode | Execution | Description |
|------|-----------|-------------|
| `interactive` | `cl` directly | Full terminal passthrough; calls `cl --verbose [--session-id UUID] [--append-system-prompt ...] @{prompt_path}`, inheriting stdio |
| `afk` | ralph | Autonomous execution; ralph invokes `cl` with `--dangerously-skip-permissions`, NDJSON stream formatting |

**Interactive mode**: Calls `cl` directly. No PID file, no log tee. Generates a loop_id and writes session metadata to `.sgf/run/{loop_id}.json` for resume capability. `cl` handles context file injection (MEMENTO, BACKPRESSURE, specs/README). When a spec is provided, sgf passes `--append-system-prompt 'study @./specs/<stem>.md'` to `cl`. When `auto_push` is true, auto-pushes after the session if HEAD changed. Passes `--session-id <uuid>` to `cl` for session tracking.

**AFK mode**: Goes through ralph, which invokes `cl` directly on the host. Ralph passes spec study args via `--append-system-prompt`; `cl` handles context file injection. PID file, log tee, and loop ID are managed by sgf. Session metadata (`.sgf/run/{loop_id}.json`) is written before spawn and updated on exit.

#### Session Metadata

For both modes, sgf generates a session UUID before spawning and writes session metadata to `.sgf/run/{loop_id}.json`. The metadata includes the session ID, loop config, and status. On exit, the status is updated based on exit code (`completed`, `interrupted`, `exhausted`). See [session-resume spec](session-resume.md) for the full schema.

#### Auto-push for interactive commands

Interactive commands with `auto_push = true` auto-push after the Claude session exits using `vcs_utils::auto_push_if_changed()` from the shared [vcs-utils](vcs-utils.md) crate: capture `vcs_utils::git_head()` before the session, then call `auto_push_if_changed()` after. Push failures are non-fatal (logged as warnings). Suppressed with `--no-push`.

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|----|
| `0` | Sentinel found (`.ralph-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

Interrupt handling uses the shared `shutdown` crate's `ShutdownController` (see [shutdown spec](shutdown.md)). The controller configuration depends on the mode:

**AFK mode** (`sgf build -a`, `sgf verify -a`, etc.): sgf spawns ralph in its own session (`setsid()` via `pre_exec`) with `Stdio::null()` for stdin. Stdin isolation prevents the agent from inheriting the terminal fd and modifying terminal settings (e.g., disabling ISIG via `tcsetattr`), which would cause Ctrl+C/Ctrl+D to emit raw bytes instead of generating signals/EOF. The controller is created with `monitor_stdin: true` — stdin is free since no user interaction occurs. Both double Ctrl+C (SIGINT) and double Ctrl+D (stdin EOF) trigger shutdown. First press prints "Press Ctrl-C again to exit" (or "Press Ctrl-D again to exit") to stderr. Second press of the same key within 2 seconds: sgf kills ralph's process group via `shutdown::kill_process_group(pid, 200ms)` (SIGTERM to group, wait up to 200ms, escalate to SIGKILL), waits for exit, returns code 130. Timeout resets the counter. SIGTERM always triggers immediate shutdown (single signal).

**Non-AFK mode** (`sgf build`, `sgf verify`, etc.): sgf spawns ralph **without** `setsid()` — ralph and the agent stay in sgf's process group, receiving terminal signals naturally and retaining full terminal access. The controller is created with `monitor_stdin: false` — stdin belongs to the child for user interaction with Claude. Only double Ctrl+C works for shutdown; Ctrl+D goes to Claude as normal input. Both sgf and the child receive SIGINT on Ctrl+C; sgf's handler prints the confirmation prompt while Claude handles the signal with its own logic.

**Interactive stages** (`sgf spec`, `sgf issues log`): Same as non-AFK — no `setsid()`, `monitor_stdin: false`. The user types directly into Claude.

sgf sets `SGF_MANAGED=1` in ralph's environment so ralph disables its own stdin monitoring and relies on sgf for Ctrl+D detection (AFK) or passes stdin through (non-AFK). Ralph still handles SIGTERM from sgf for graceful cleanup.

Signal handlers are registered just before spawning the child — during pre-launch checks, daemon startup, and other phases before handler registration, default signal behavior applies (single SIGINT exits).

Claude Code crashes and push failures are handled within ralph as warnings — they do not produce distinct exit codes. Ralph logs the failure and continues to the next iteration without cleanup. The next iteration's agent inherits whatever state exists and proceeds via forward correction. Stale claims and dirty working trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery before the next run.

### Completion Sentinel

The agent creates a `.ralph-complete` file when `pn ready` returns no tasks. Ralph checks for this file after each iteration. If found, ralph deletes it, performs a final auto-push (if enabled), and exits with code `0`.

---

## Loop ID Format

## Loop ID Format

`sgf` generates loop IDs with the pattern: `<stage>[-<spec>]-<YYYYMMDDTHHmmss>`

Examples:
- `build-auth-20260226T143000` (build loop for auth spec)
- `verify-20260226T150000` (verify loop, no spec filter)
- `issues-plan-20260226T160000` (issues plan loop)

Ralph includes the loop ID in log output. `sgf logs` uses the loop ID to locate log files.

---

## Logging

## Logging

`sgf` tees ralph's stdout to both the terminal and `.sgf/logs/<loop-id>.log`. Ralph owns formatting — in AFK mode it emits human-readable one-liners (tool calls, text blocks); in interactive mode it passes through the terminal. `sgf` does not parse ralph's output.

The `.sgf/logs/` directory is gitignored.

### sgf logs

`sgf logs <loop-id>` runs `tail -f .sgf/logs/<loop-id>.log`. If the log file does not exist, print an error and exit 1.

---

## Console Output

## Console Output

sgf uses a rounded-box badge for all status output to stderr. Every message is wrapped in a 3-line box drawn with Unicode box-drawing characters (`╭╮╰╯│─`), echoing ralph's rounded-box aesthetic. The `sgf` label appears on the middle line in bold. The box borders are dim. Message text sits to the right of the box on the middle line — its color conveys semantic state.

### Visual Format

Each message gets its own 3-line box. The box is always 7 characters wide (`╭─────╮`). The `sgf` label is centered inside on the middle line in bold. The message text appears to the right of the closing `│` on the middle line.

```
╭─────╮
│ sgf │ launching ralph [build-auth-20260312T143000]
╰─────╯ stage: auth · iterations: 10 · mode: afk
╭─────╮
│ sgf │ recovering from stale state...
╰─────╯
╭─────╮
│ sgf │ recovery complete
╰─────╯
╭─────╮
│ sgf │ starting pensa daemon...
╰─────╯
╭─────╮
│ sgf │ pensa daemon ready
╰─────╯
╭─────╮
│ sgf │ pn export ok
╰─────╯
╭─────╮
│ sgf │ pushing → origin/main...
╰─────╯
╭─────╮
│ sgf │ loop complete [build-auth-20260312T143000]
╰─────╯
╭─────╮
│ sgf │ ralph exited with error [build-auth-20260312T143000]
╰─────╯
╭─────╮
│ sgf │ iterations exhausted [build-auth-20260312T143000]
╰─────╯
```

### Color Scheme

| State | Message Color | When |
|-------|---------------|------|
| Action | White | In-progress operations: launching, recovering, pushing, starting daemon |
| Success | Green | Completed operations: recovery complete, daemon ready, pn export ok, loop complete, pushed |
| Warning | Yellow | Non-fatal issues: pn export skipped, pn doctor failed, iterations exhausted |
| Error | Red | Fatal failures: ralph exited with error, pn export failed |
| Detail | Dim (gray) | Supplementary info: stage, iterations, mode (below box, no badge) |

The box borders (`╭─────╮`, `│`, `╰─────╯`) are always **dim**. The `sgf` text inside the box is always **bold** (`\x1b[1m sgf \x1b[0m`) — normal text color regardless of message state.

### Box Construction

The badge box is 3 lines emitted to stderr:

1. **Top**: `dim(╭─────╮)`
2. **Middle**: `dim(│) bold( sgf ) dim(│)` + space + colored message
3. **Bottom**: `dim(╰─────╯)` + optional detail text

The box is stateless — each semantic output call (`print_action`, `print_success`, etc.) emits its own complete 3-line box. No buffering or grouping.

### Detail Lines

Detail lines appear on the bottom line of the box, to the right of `╰─────╯`, aligned with the message text on the middle line (8 characters: 7-char box width + 1-space gap). They are rendered in dim gray.

Detail lines appear for:
- **Ralph launch**: `stage: <spec> · iterations: <n> · mode: <afk|interactive>`
- **Interactive launch**: `stage: <stage>`

### NO_COLOR Support

When the `NO_COLOR` environment variable is set, all ANSI codes and box-drawing characters are suppressed. The badge falls back to plain `sgf:` prefix. Detail lines are indented with plain spaces. Message text has no color formatting.

```
sgf: launching ralph [build-auth-20260312T143000]
     stage: auth · iterations: 30
sgf: recovery complete
sgf: ralph exited with error [build-auth-20260312T143000]
```

### style.rs Module

`crates/springfield/src/style.rs` provides styling primitives and semantic output functions. Mirrors ralph's `style.rs` structure for ANSI primitives but adds sgf-specific badge box and message functions.

**ANSI Primitives** (same interface as ralph):
- `bold(s)`, `dim(s)`, `green(s)`, `yellow(s)`, `red(s)`, `white(s)`
- `no_color()` — checks `NO_COLOR` environment variable
- `strip_ansi(s)` — removes ANSI escape sequences

**Badge Box**:
- `badge_top()` — returns the top border: `dim(╭─────╮)`
- `badge_mid()` — returns the middle line badge: `dim(│) bold( sgf ) dim(│)`
- `badge_bot()` — returns the bottom border: `dim(╰─────╯)`

**Semantic Output** (all write to stderr via 3-line box):
- `action(msg)` — box + bold white message
- `success(msg)` — box + bold green message
- `warning(msg)` — box + bold yellow message
- `error(msg)` — box + bold red message
- `detail(msg)` — indented dim message, no box (appended to bottom line of preceding box)

### Auto-push Callback

The `vcs_utils::auto_push_if_changed()` callback emits raw messages (e.g., `"New commits detected, pushing..."`, `"push failed (non-fatal): ..."`). The callback in `orchestrate.rs` wraps these with the appropriate styled output function — action for "pushing", warning for "push failed".

### Message Catalog

Every `eprintln!("sgf: ...")` and `println!(...)` call in the springfield crate is replaced with a styled output call.

| Message | Style | Source |
|---------|-------|--------|
| recovering from stale state... | action | recovery.rs |
| recovery complete | success | recovery.rs |
| pn doctor --fix exited with {status} | warning | recovery.rs |
| pn doctor --fix failed: {e} | warning | recovery.rs |
| starting pensa daemon on port {port}... | action | recovery.rs |
| starting forma daemon on port {port}... | action | recovery.rs |
| pensa daemon ready | success | recovery.rs |
| forma daemon ready | success | recovery.rs |
| pn export ok | success | orchestrate.rs |
| pn export failed: {err} | error | orchestrate.rs |
| pn export skipped (pn not found: {e}) | warning | orchestrate.rs |
| launching interactive session [{stage}] | action | orchestrate.rs |
| launching ralph [{loop_id}] | action | orchestrate.rs |
| loop complete [{loop_id}] | success | orchestrate.rs |
| ralph exited with error [{loop_id}] | error | orchestrate.rs |
| iterations exhausted [{loop_id}] | warning | orchestrate.rs |
| interrupted [{loop_id}] | warning | orchestrate.rs |
| ralph exited with unexpected code [{loop_id}] | error | orchestrate.rs |
| New commits detected, pushing... | action | orchestrate.rs (auto-push callback) |
| push failed (non-fatal): {err} | warning | orchestrate.rs (auto-push callback) |
| .sgf/MEMENTO.md not found — agents won't have fm/pn workflow reference | warning | init.rs |
| .sgf/BACKPRESSURE.md not found — agents won't have build/test/lint reference | warning | init.rs |
| project scaffolded successfully | success | init.rs |
| {stage}: {e} | error | main.rs |

---

## Recovery

## Recovery

Ralph does not perform iteration-start cleanup. Recovery is `sgf`'s responsibility, executed before launching ralph.

### PID Files

`sgf` writes `.sgf/run/<loop-id>.pid` on launch (containing its process ID) and removes it on clean exit. The `.sgf/run/` directory is gitignored.

### Pre-launch Cleanup

Before launching ralph, `sgf` scans all PID files in `.sgf/run/`:

- **Any PID alive** (verified via `kill -0`) → another loop is running. Skip cleanup and launch normally — the dirty tree or in-progress claims may belong to that loop.
- **All PIDs stale** (process dead) → no loops are running. Remove stale PID files, then recover:
  1. `git checkout -- .` — discard modifications to tracked files. **Failure is fatal** — loop launch is aborted.
  2. `git clean -fd` — remove untracked files (respects `.gitignore`, so `db.sqlite` and logs are safe). **Failure is fatal** — loop launch is aborted.
  3. `pn doctor --fix` — release stale claims and repair integrity (warning only — supplementary, not critical for state consistency)

**Principle**: Work is only preserved when committed. Uncommitted changes from crashed iterations are discarded — the agent that produced them is gone and cannot continue them. Git recovery failures are hard errors that prevent loop launch — proceeding with dirty state would violate the atomic iteration guarantee.

---

## Pre-launch Lifecycle

## Pre-launch Lifecycle

Before launching any loop, `sgf` runs pre-launch checks. The checks vary by stage:

**All stages** (build, verify, test-plan, test, spec, issues log):

1. **Recovery** — clean up stale state from crashed iterations (see Recovery)
2. **Daemons** — start the pensa and forma daemons if not already running

After pre-launch checks, automated stages launch ralph; interactive stages call `cl` directly with `--verbose @{prompt_path}`, inheriting stdio.

**`SGF_SKIP_PREFLIGHT`** (env var) — When set, skips daemon startup while still running recovery. This allows two-tier control: the `--skip-preflight` CLI flag disables all pre-launch checks (including recovery), while `SGF_SKIP_PREFLIGHT` disables only the infrastructure checks (daemon). Used by integration tests to exercise recovery logic without requiring a running pensa daemon.

### Daemons

`sgf` starts both the pensa and forma daemons automatically before launching any loop (if not already running):

#### Port derivation

Each daemon uses its own port derivation to avoid collisions:

- **Pensa**: `SHA256(canonical_project_path)`, bytes [8,9] mapped to range [10000, 60000]
- **Forma**: `SHA256("forma:" + canonical_project_path)`, bytes [8,9] mapped to range [10000, 60000]

The `"forma:"` prefix ensures forma and pensa derive different ports for the same project. Springfield's `pensa_port()` and `forma_port()` in `recovery.rs` must match the derivation logic in each daemon's own crate.

#### Pensa daemon

1. Check if the daemon is reachable (`pn daemon status`)
2. If not, start it: `pn daemon --project-dir <project-root> --port <pensa-derived> &` (backgrounded)
3. Wait for readiness (poll `pn daemon status` with exponential backoff: 50ms initial, doubling up to 800ms cap, 5s total deadline)

#### Forma daemon

1. Check if the daemon is reachable (`fm daemon status`)
2. If not, start it: `fm daemon --project-dir <project-root> --port <forma-derived> &` (backgrounded)
3. Wait for readiness (poll `fm daemon status` with exponential backoff: 50ms initial, doubling up to 800ms cap, 5s total deadline)

Both daemons are started in parallel. Both must be ready before proceeding with loop launch. The daemons run for the duration of the `sgf` session. They stop on SIGTERM or when `sgf` shuts down.

---

## Workflow Stages

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready --spec <stem>` returns nothing (all tasks for a spec are done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., multiple `sgf build` instances) can run concurrently on the same branch. The pensa daemon serializes all database access, providing atomic claims via `pn update --claim` (fails with `already_claimed` if another agent got there first). `pn export` runs at commit time via the pre-commit hook. Concurrent agents share the same filesystem and git history. **Stop build loops before running `sgf spec`** to avoid task-supersession race conditions.

### Standard Loop Iteration

Build, Test, and Issues Plan stages share a common iteration pattern. Each iteration:

1. **Orient** — context files (MEMENTO, BACKPRESSURE, specs/README) are injected by `cl` via `study` instructions; spec files are injected by ralph via `--spec`.
2. **Query** — find work items via pensa (stage-specific query). If none, write `.ralph-complete` and exit.
3. **Choose & Claim** — pick a task from the results, then `pn update <id> --claim`. If the claim fails (`already_claimed`), re-query and pick another.
4. **Work** — stage-specific implementation
5. **Log issues** — if problems are discovered: `pn create "description" -t bug`
6. **Close/release** — close or release the work item
7. **Commit** — prefix the commit message with `[<task-id>]` (e.g., `[pn-a1b2c3d4] Implement login validation`). The pre-commit hook runs `pn export` automatically, syncing SQLite to JSONL. The prefix enables `git log --grep` for per-task history.

Each iteration gets fresh context. The pensa database persists state between iterations.

| Stage | Query | Work | Close |
|-------|-------|------|-------|
| Build | `pn ready [--spec <stem>] --json` | Implement the task (or plan the bug — see below); apply backpressure | `pn close <id> --reason "..."` (tasks) / `pn release <id>` (bugs) |
| Test | `pn ready -t test [--spec <stem>] --json` | Execute the test | `pn close <id> --reason "..."` |

#### Bug Handling in the Build Loop

`pn ready` includes unplanned bugs (see pensa spec). When the build loop claims a bug, the agent studies the codebase then decides how to proceed:

**Small bugs (fixable in this iteration):** Fix it directly — implement, test, apply backpressure, and close the bug. Treat it like a normal task.

**Large bugs (multiple files/crates, significant refactor):** Decompose into fix tasks:

1. Create fix task(s): `pn create -t task "fix: <description>" --fixes <bug-id> [--spec <stem>] [-p <priority>] [--dep <id>]`
2. Comment lessons learned on the bug: `pn comment add <bug-id> "..."`
3. Release the bug: `pn release <bug-id>` (the bug drops out of `pn ready` — it now has fix children)
4. Commit with `[<bug-id>]` prefix

The fix tasks appear in subsequent `pn ready` calls and are implemented as normal tasks. When all fix tasks for a bug are closed, pensa auto-closes the bug.

### 1. Spec (`sgf spec`)

Opens an interactive Claude Code session with the spec prompt. Calls `cl` directly (no ralph). The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates deliverables:

1. Create or update specs via `fm` (Spec Create and/or Spec Update Workflow from MEMENTO)
2. Create implementation plan items via `pn create -t task --spec <stem>`, with dependencies and priorities
3. Commit and push

The interview and generation happen in a single session. The agent asks clarifying questions as needed, but the goal is always to produce specs and a plan. The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line.

Tasks linked to a spec *are* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Spec revision**: Run `sgf spec` again. **Stop any running build loops before revising specs.** When revising, the agent:
1. Reviews existing tasks for the spec: `pn list --spec <stem> --json`
2. Closes tasks that are no longer relevant: `pn close <id> --reason "superseded by revised spec"`
3. Creates new tasks for the delta: `pn create "..." -t task --spec <stem>`
4. Updates the spec via `fm`
5. Restart build loops after revision is committed

### 2. Build (`sgf build [spec]`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/build.md`. The spec stem is **optional** — `sgf build auth` builds tasks for the `auth` spec, while `sgf build` runs without a spec filter. When a spec is provided, sgf validates that `specs/auth.md` exists before launching (fails with a clear error if not found).

When a spec is given, sgf sets `SGF_SPEC=auth` in ralph's environment and passes `--spec auth` to ralph. Ralph includes `specs/auth.md` in its `study` instruction so the agent actively reads the full spec. When no spec is given, neither `SGF_SPEC` nor `--spec` are set. The build stage adds **backpressure** — after implementing the task, the agent runs build, test, and lint commands per `BACKPRESSURE.md`.

Run interactively first for a few supervised rounds, then switch to AFK mode (`-a`) for autonomous execution.

### 3. Verify (`sgf verify`)

Runs via ralph using `.sgf/prompts/verify.md`. Each iteration handles one spec:

1. List all specs via `fm list --json`
2. Pick one unverified spec and investigate it against the codebase (read via `fm show <stem> --json`)
3. Mark conformance: ✅ Matches spec, ⚠️ Partial match, ❌ Missing/different
4. Update `verification-report.md`
5. Log any gaps as pensa bugs: `pn create "..." -t bug`
6. Commit

When all specs have been verified, write `.ralph-complete`.

### 4. Test Plan (`sgf test-plan`)

Runs via ralph using `.sgf/prompts/test-plan.md`. The agent:

1. Studies specs and codebase
2. Generates a testing plan
3. Ensures tests are automatable (can be run by agents in loops)
4. Creates test items via `pn create -t test --spec <stem>`, with dependencies and priorities
5. Commits

### 5. Test (`sgf test [spec]`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/test.md`. The spec stem is **optional** — `sgf test auth` runs test items for the `auth` spec, while `sgf test` runs all test items regardless of spec. When a spec is provided, sgf validates that `specs/auth.md` exists before launching. Sets `SGF_SPEC` and `--spec` only when a spec is given.

After all test items are closed, a final iteration generates `test-report.md` — a summary of all test results, pass/fail status, and any bugs logged.

### 6. Issues Log (`sgf issues-log`)

Calls `cl` directly (no ralph) using `.sgf/prompts/issues-log.md`. Each session handles one bug:

1. The developer describes a bug they've observed
2. The agent interviews them to capture details — steps to reproduce, expected vs actual behavior, relevant context
3. Logs the bug via `pn create -t bug`

One bug per session. The developer runs `sgf issues log` again for additional bugs — fresh context each time prevents accumulation across unrelated issues.

### 7. Doc (`sgf doc`)

Calls `cl` directly (no ralph) using `.sgf/prompts/doc.md`. Runs `pn doctor --json` and triages the results:

1. Run `pn doctor --json`
2. For each reported issue, investigate whether it has been completed or is still valid
3. Comment pertinent findings on affected issues
4. Close any completed or invalid issues

Auto-pushes after the session if HEAD changed (like `sgf spec`). Suppressed with `--no-push`.

### 8. Inline Issue Logging

Issues are also logged by agents during any stage via `pn create`. The build loop logs bugs it discovers during implementation. The verify loop logs spec gaps. The test loop logs test failures. `sgf issues log` is for developer-reported bugs; inline logging is for agent-discovered bugs.

---

## Shipped Prompts

## Shipped Prompts

Each command has a corresponding prompt file. The defaults live in `~/.sgf/prompts/` (synced from the springfield repo's `.sgf/prompts/` via `just install`). Override any prompt per-project by creating `./.sgf/prompts/<name>.md`.

| Prompt | Purpose |
|--------|---------|
| `spec.md` | Interactive spec discussion and implementation planning |
| `build.md` | Claim one pn issue, implement it, apply backpressure, commit |
| `verify.md` | Verify one spec against codebase, update verification report |
| `test-plan.md` | Generate test items from specs using pn |
| `test.md` | Claim one pn test item, execute it, apply backpressure |
| `issues-log.md` | Interactive bug reporting session |
| `doc.md` | Interactive pensa doctor triage |

The canonical prompts live in the springfield repo's `.sgf/prompts/` — do not duplicate their contents here.

### Custom Prompts

Users can add custom prompts by creating a new `.md` file in `./.sgf/prompts/` (project-local) or `~/.sgf/prompts/` (global) and optionally adding a `[section]` in `config.toml`. For example, adding `deploy.md` and `[deploy]` in config.toml enables `sgf deploy`. Without a config.toml entry, `sgf deploy` still works with fallback defaults (interactive, 1 iteration, no auto-push).

---

## Backpressure

## Backpressure

`BACKPRESSURE.md` lives in the springfield repo's `.sgf/` directory and is synced to `~/.sgf/` via `just install`. It contains universal build, test, lint, and format commands for common project types. The developer deletes sections that don't apply to their project by creating a project-local override in `./.sgf/BACKPRESSURE.md`.

---

## Defaults

## Defaults

Per-command defaults live in `.sgf/prompts/config.toml` (see [Prompt Configuration](#prompt-configuration)). CLI flags override config.toml values:

| Setting | Fallback Default | Override |
|---------|-----------------|----------|
| Mode | `interactive` | `-a` / `-i` flags |
| Iterations | `1` | `-n` / `--iterations` |
| Auto-push | `false` | `--no-push` flag (disables), config.toml `auto_push` field |
| Pensa daemon port | per-project derived (`SHA256(path)`) | `--port` flag on `pn daemon` |
| Forma daemon port | per-project derived (`SHA256("forma:" + path)`) | `--port` flag on `fm daemon` |

---

## Key Design Principles

## Key Design Principles

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt enforces: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**One task, fresh context**: Each iteration picks one unblocked task, implements it fully, commits, and exits. The loop restarts with a clean context window. No accumulated confusion, no multi-task sprawl.

**Atomic iterations**: An iteration either commits fully or is discarded entirely. Partial work from crashed iterations is never preserved — sgf's pre-launch recovery wipes uncommitted state before the next run.

**Structured memory over markdown**: Pensa replaces markdown-based issue logging and plan tracking. A single CLI command replaces the error-prone multi-step process of creating directories and writing files. `pn` is the exclusive task tracker — agents must never use TodoWrite, TaskCreate, or markdown files for tracking work.

**Tasks as implementation plan**: There is no separate "implementation plan" entity. The living set of pensa tasks linked to a spec *is* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Editable prompts**: Prompts are plain markdown files. Global defaults live in `~/.sgf/prompts/` (synced from the springfield repo). Override per-project by creating `./.sgf/prompts/<name>.md`. Drop new `.md` files into either location to create new commands — no code changes required. To improve defaults for all projects, edit the files in the springfield repo's `.sgf/` and run `just install`.

**Layered context injection**: `cl` (claude-wrapper) resolves context files (MEMENTO.md, BACKPRESSURE.md, specs/README.md) via layered `.sgf/` lookup (local `./.sgf/` → global `~/.sgf/`) and injects them as `study` instructions into every Claude session. This applies uniformly to both interactive and automated stages. sgf does not inject context — it validates and resolves prompt paths, then delegates to `cl` or ralph.

**Protected scaffolding**: `.sgf/` and `.claude/` are protected from agent writes via Claude deny settings. The developer is the authority on prompts, settings, and project configuration.

**Layered projects**: Springfield uses two-tier `.sgf/` resolution — project-local `./.sgf/` overrides global `~/.sgf/` on a file-by-file basis. Projects only need local overrides for project-specific customizations; everything else falls through to the global defaults.

**Direct execution with native sandbox**: All stages invoke `cl` on the host — no Docker sandboxes, no Mutagen sync. Claude Code's native sandbox (Seatbelt on macOS, bubblewrap on Linux) provides OS-level filesystem and network isolation, enabled by default via `.claude/settings.json`. Automated stages go through ralph with `--dangerously-skip-permissions` and `sandbox.allowUnsandboxedCommands: false` — agents operate freely within sandbox bounds but cannot escape. Interactive stages use the sandbox with `allowUnsandboxedCommands: true` so developers can approve out-of-sandbox commands when needed.

---

## Future Work

## Future Work

- **Context-efficient backpressure**: Swallow all build/test/lint output on success (show only a checkmark), dump full output only on failure. Preserves context window budget. See HumanLayer's `run_silent()` pattern.
- **Claude Code hooks for enforcement**: Use `PreToolUse` / `PostToolUse` hooks to enforce backpressure at the framework level — auto-run linters after file edits, block destructive commands. Could be scaffolded by `sgf init`.
- **TUI**: CLI-first for now. TUI can be added later as a view layer. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.
- **`sgf status` output spec**: Define what `sgf status` shows (running loops, pensa summary, recent activity). Specify after real usage reveals what's needed.

## Per-Repo Project Structure

After `sgf init` and ongoing development, a project contains:

```
.pensa/
├── db.sqlite                  (gitignored — daemon-owned working database)
├── issues.jsonl               (committed — git-portable export)
├── deps.jsonl                 (committed)
└── comments.jsonl             (committed)
.sgf/
├── MEMENTO.md                 (fm/pn workflow reference — authored per-project)
├── BACKPRESSURE.md            (build/test/lint/format reference — authored per-project)
├── logs/                      (gitignored — AFK loop output)
│   └── <loop-id>.log
├── run/                       (gitignored — PID files for running loops)
│   └── <loop-id>.pid
└── prompts/                   (optional — project-local overrides only)
    └── build.md               (example: overrides just build.md, other prompts fall through to ~/.sgf/)
.pre-commit-config.yaml        (prek hooks for pensa sync)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (`ln -s` to AGENTS.md)
test-report.md                 (generated — overwritten each test run, committed)
verification-report.md         (generated — overwritten each verify run, committed)
specs/
├── README.md                  (agent-maintained spec index — loom-style tables)
└── *.md                       (prose specification files)
```

### Global Home Structure

Populated by `just install` (rsync from the springfield repo's `.sgf/`):

```
~/.sgf/
├── MEMENTO.md                 (universal agent instructions — fm/pn workflows, conventions)
├── BACKPRESSURE.md            (universal build/test/lint/format reference)
└── prompts/
    ├── config.toml            (per-command defaults: mode, iterations, auto_push, alias)
    ├── build.md               (default prompts for all projects)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    ├── issues-log.md
    └── doc.md
```

### Installation

All crates are installed via `just install`, which also syncs the global `~/.sgf/` directory:

```just
install:
    cargo install --path crates/pensa
    cargo install --path crates/ralph
    cargo install --path crates/springfield
    cargo install --path crates/claude-wrapper
    rsync -av --delete --exclude='logs/' --exclude='run/' .sgf/ ~/.sgf/
```

The rsync copies prompts, config, MEMENTO.md, and BACKPRESSURE.md to `~/.sgf/`. The `--delete` flag removes files from `~/.sgf/` that no longer exist in the repo. Runtime directories (`logs/`, `run/`) are excluded.

### File Purposes

**`~/.sgf/BACKPRESSURE.md`** — Universal build, test, lint, and format commands. Developer-editable. Override per-project by placing a `BACKPRESSURE.md` in `./.sgf/`. Injected into every Claude session by `cl` (see [claude-wrapper spec](claude-wrapper.md)).

**`~/.sgf/MEMENTO.md`** — Universal agent instructions (fm/pn workflows, conventions, sandbox rules). Override per-project by placing a `MEMENTO.md` in `./.sgf/`. Injected into every Claude session by `cl`.

**`AGENTS.md`** — Hand-authored operational guidance. Contains code style preferences, runtime notes, and special instructions. Created as an empty file by `sgf init`.

**`CLAUDE.md`** — Entry point for Claude Code. Symlinks to AGENTS.md. Auto-loaded by Claude Code at the start of every session.

**`config.toml`** — Per-command defaults. Defines `mode`, `iterations`, `auto_push`, and optional `alias` for each prompt. Lives in `~/.sgf/prompts/` (global) with optional per-project override in `./.sgf/prompts/config.toml`. Local config sections override global ones by key; global sections not overridden locally are preserved. See [Prompt Configuration](#prompt-configuration).

**`~/.sgf/prompts/`** — Default prompts for all projects. Synced from the springfield repo via `just install`. To override a prompt for a specific project, create `./.sgf/prompts/<name>.md` — that file takes precedence for that project only. Adding a new `.md` file to either location makes it available as `sgf <name>` immediately (with fallback defaults if no config.toml entry exists).

**`.sgf/` and `.claude/` protection** — Both `.sgf/` and `.claude/` are protected from agent modification via Claude deny settings. `sgf init` scaffolds these rules. `.sgf/` protection prevents agents from modifying local overrides and reference files. `.claude/` protection prevents agents from weakening sandbox configuration or deny rules.

**`SGF_SPEC`** (env var) — Spec stem for build/test stages. Set by sgf in ralph's environment (e.g., `SGF_SPEC=auth`). Ralph includes `./specs/${SGF_SPEC}.md` in its `study` instruction. Prompt files reference this env var directly (e.g., `$SGF_SPEC`).

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [forma](forma.md) — Specification management — forma daemon and fm CLI
- [pensa](pensa.md) — Agent persistent memory — SQLite-backed issue/task tracker with pn CLI
- [ralph](ralph.md) — Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push
- [shutdown](shutdown.md) — Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts
- [vcs-utils](vcs-utils.md) — Shared VCS utilities — git HEAD detection, auto-push
