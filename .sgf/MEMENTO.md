## Rules

These rules override default behavior. Follow them exactly.

- **Relative paths only** — use paths from the repo root for file operations, not absolute paths.
- **`pn`, never `gh`** — issues live in `pn`, not `gh`. ALWAYS use `pn` for bugs and issues.
- **No bare `tsx`** — the Claude Code sandbox blocks the IPC pipe. Use `node --import tsx/esm <script>` instead.
- **Edit settings in dotfiles** — always edit `~/Repos/dotfiles/.claude` (not `~/`).
- **Subagent limits** — do not spawn more than 3 concurrent Agent/subagent calls. Large refactors must be done sequentially.
- **Session start** — run `fm list --json` at the beginning of EACH SESSION.
- **Uncommitted changes** — if `git status` shows a dirty working tree at session start, check the most recent `.sgf/logs` entry to understand what produced them before asking the user what to do with them. They are usually formatter residue from backpressure and should likely be committed.
- **MCP tools** — use MCP tools instead of running commands directly via Bash when a wrapper exists. They bypass sandbox restrictions.
- **Making config changes** — always make changes in `~/Repos/dotfiles` — never edit the deployed copy directly.
- **Prompts** — frequently used prompts live in `~/.agents/prompts/*.md`. If asked to study something or reference something, look there.
- **MANDATORY voice output** — Every response you send to the user MUST end with a `run_dic` MCP tool call (run in background) speaking a brief summary. See `Voice Output` section for exceptions.

### MCP Tools

| Tool | Purpose |
|------|---------|
| `run_playwright` | Run Playwright e2e tests. Chromium can't launch from Bash. Pass `config` for non-default configs (e.g. `playwright-visual.config.ts`). |
| `create_project` | Scaffold new projects (e.g. `pnpm create vite`, `npm create next-app`). Never run scaffold commands via Bash — they are blocked by the sandbox. |
| `run_pnpm` | Run allowlisted pnpm scripts that need network access (e.g. `seed`, `push:schema`, `push:perms`). Loads `.env` from the project root and strips proxy env. To add new scripts, update `ALLOWED_PNPM_SCRIPTS` in the unsandboxed-runner source. |
| `run_dic` | Speak text aloud via the `dic` TTS wrapper. Accepts `text`, optional `voice` (default: bf_isabella), and optional `speed`. |
| `run_newsboat` | Run newsboat commands outside the sandbox for RSS/Atom feed access. |
| `run_kw` | Run the kw CLI outside the sandbox for keyword research API access. |
| `save_config` | Deploy dotfiles from `~/Repos/dotfiles` to `$HOME`. Use after editing config files in dotfiles. |

### Shortcuts

| Shortcut | Meaning |
|----------|---------|
| `cp`     | Commit and push |

### Invoking `sgf` programmatically

When calling `sgf` commands from within a Claude Code session (e.g., via Bash tool), **pipe the message through stdin** — do NOT pass it as a positional argument. The positional argument is reserved for spec stems (e.g., `sgf c auth`), not free-text descriptions. Piped stdin activates programmatic mode, which emits structured NDJSON events.

**NEVER use heredoc syntax.** Heredocs do not survive `sh -c` — newlines get mangled, the terminator is never found, and stdin arrives empty. Use `echo` or `printf` instead.

```bash
# Correct:
echo "Fix the settings button visibility" | sgf c

# Also correct (printf for content with special chars):
printf '%s\n' "Fix the settings button visibility" | sgf c

# WRONG — treats the string as a spec stem:
sgf c "Fix the settings button visibility"

# WRONG — heredocs break under sh -c:
cat <<'EOF' | sgf c
Fix the settings button visibility
EOF
```

### Build context

- Logs for what has been built are kept in `./.sgf/logs`. Check there when in need of context regarding what has been built.
- The most recent log by mtime is the most recent work — always start there regardless of filename prefix (e.g. `cohere-*`, `changes-*`).


## pn — Issue Tracker

`pn` (pensa) is the exclusive issue (i.e. work item) tracker. Never use TodoWrite, TaskCreate, or markdown files for tracking work.

### Rules

- Always pass `--json` when reading data.
- There is NO `pn claim` subcommand. Use `pn update <id> --claim`.
- Status values use **underscores**: `open`, `in_progress`, `closed`. Never use hyphens (`in-progress` is invalid).

### Issue Create Workflow

1. Create the issue linked to its spec:
   `pn create "<title>" -t <type> --spec <stem> [-p <priority>] [--dep <id>] [--description "<desc>"]`
   a. NOTE: Issues should be scoped to atomic changes—the smallest self-contained modifications to the codebase that can be implemented and tested independently.
2. Attach source code references (files to view/change/add):
   `pn src-ref add <id> <path> --reason "<what and why>"`
3. Attach documentation references (docs to view/change/add):
   `pn doc-ref add <id> <path> --reason "<what and why>"`

### Issue Claim Workflow

1. Query for issues (e.g., `pn ready --json` or `pn ready --spec auth --json`).
  a. IMPORTANT: **Do NOT run `pn list` to see open issues. Use `pn ready`.**
2. **If there are no issues returned from `pn ready`, there are no available issues to claim right now.**
3. Pick ONE issue and claim: `pn update <id> --claim`.
4. If claim fails (`already_claimed`) → re-query and pick another.
  a. NOTE: Do NOT work on an already-claimed issue.
    i. (Even if it is claimed under your name.)

### Issue Close Workflow

1. Comment on the issue (`pn comment add <id> "<insights>"`):
  a. crucial, useful lessons learned (if any)
  b. notable design/testing decisions made (if any)
  c. root cause of issue (if applicable)
2. Close or release:
  a. IF BUG that has NOT yet been fixed: release with `pn release <bug-id>`
  b. ELSE: close with `pn close <id> --reason "<what was done>"`
3. Commit YOUR changes with `[<issue-id>]` prefix (e.g., `[pn-a1b2c3d4] Implement login validation`)

### Bug Log Workflow

`pn create "<description>" -t bug`

### Bug Fix Workflow

- Study the codebase to understand the bug.
- IF the fix is small enough to quickly implement in this iteration:
  1. Fix it.
  2. Follow the Spec Update Workflow as appropriate.
- ELSE IF the fix is too large (multiple files/crates, significant refactor):
  1. Follow the Issue Create Workflow to create implementation items.
    a. Link the relevant bug with `--fixes <bug-id>`.
- Follow Issue Close Workflow.

### Core Commands

| Command | Purpose |
|---------|---------|
| `pn ready [--spec <stem>] [-t <type>] --json` | List unblocked, unclaimed work items |
| `pn list [-t <type>] [--status <s>] [--spec <stem>] --json` | List items with filters |
| `pn blocked --json` | List blocked items |
| `pn show <id> --json` | Show item details |
| `pn search "<query>" --json` | Full-text search across issues |
| `pn count [--by-status] [--by-priority] [--by-issue-type] [--by-assignee] --json` | Count/summarize items |
| `pn status --json` | Project status overview |
| `pn create "<title>" -t <type> [--spec <stem>] [-p <priority>] [--dep <id>] [--fixes <id>] [--description <desc>]` | Create item (types: task, test, bug, chore) |
| `pn update <id> --claim` | Atomically claim an item |
| `pn update <id> --unclaim` | Release a claim |
| `pn update <id> --assignee <name>` | Set assignee (not `--assign`) |
| `pn update <id> --status <status>` | Set status: `open`, `in_progress`, `closed` (underscores, not hyphens) |
| `pn close <id> --reason "<reason>" [--force]` | Close an item |
| `pn reopen <id> [--reason "<reason>"]` | Reopen a closed item |
| `pn release <id>` | Release without closing |
| `pn delete <id> [--force]` | Delete an item |
| `pn history <id> --json` | Show item change history |
| `pn comment add <id> "<text>"` | Add a comment |
| `pn comment list <id> --json` | List comments on an item |
| `pn dep add <id> --dep <other-id>` | Add a dependency |
| `pn dep remove <id> --dep <other-id>` | Remove a dependency |
| `pn dep list <id> --json` | List dependencies |
| `pn dep tree <id> --json` | Show dependency tree |
| `pn dep cycles --json` | Detect dependency cycles |
| `pn src-ref add <id> <path> [--reason "<text>"]` | Add source code reference to an issue |
| `pn src-ref list <id> --json` | List source code references |
| `pn src-ref remove <ref-id>` | Remove a source code reference |
| `pn doc-ref add <id> <path> [--reason "<text>"]` | Add documentation reference to an issue |
| `pn doc-ref list <id> --json` | List documentation references |
| `pn doc-ref remove <ref-id>` | Remove a documentation reference |

### Priorities

| Priority | Meaning | When to use |
|----------|---------|-------------|
| `p0` | Critical | Blocking all progress — broken builds, data loss, security holes |
| `p1` | High | Important and urgent — should be picked before p2/p3 work |
| `p2` | Normal | Default. Standard implementation tasks, tests, non-urgent bugs |
| `p3` | Low | Nice-to-have — polish, minor improvements, can wait indefinitely |



## fm — Specification Management

Specifications are the **source of truth** for all code. They are managed exclusively through `fm` (forma).

**IMPORTANT**: All spec mutations go through `fm`. NEVER EDIT OR VIEW SPEC MARKDOWN DIRECTLY. The generated `.forma/specs/*.md` and `.forma/README.md` are read-only artifacts for HUMANS produced by `fm export`.

### How `fm` relates to `pn`

- `pn create --spec <stem>` links an issue to a forma spec. Pensa validates the stem against forma.
- `fm check` cross-validates that all pensa issues with `--spec` values reference existing forma specs.

### Rules

- Always pass `--json` when reading data.
- Section bodies are read from **stdin** via `--body-stdin` (not as CLI arguments).
- Status values: `draft`, `stable`, `proven`.
- Specs are identified by **stem** (lowercase, alphanumeric + hyphens, e.g., `auth`, `claude-wrapper`).
- Sections are identified by **slug** (auto-generated from display name, e.g., `error-handling`).
- Required sections (`overview`, `architecture`, `dependencies`, `error-handling`, `testing`) are auto-scaffolded on `fm create` and cannot be removed.
- When passing body content to `fm section set --body-stdin`, always pipe raw content directly. Use `cat <file> |` or a Python/heredoc approach. Never use `echo "$var"` or unquoted shell expansion, as this can introduce backslash escaping artifacts.
- When updating and/or creating specs: **we are NOT documenting changes in the specs.** **INSTEAD, we are updating or writing the specs to simply reflect the new content we agreed upon.** (e.g., We want "the 'hello world' crate prints 'hello world'," instead of "instead of printing 'goodbye world,' the 'hello world' crate now prints 'hello world.'")

### Spec Create Workflow

1. Create the spec: `fm create <stem> [--src <path>] --purpose "<text>"`
  a. NOTE: Favor updating existing specs (`fm update`, `fm section set`) over creating new ones unless doing so makes sense (e.g. we're making a brand new package — use `fm create`).
2. Fill in required sections (pipe body via stdin):
   `echo "body content" | fm section set <stem> "<slug>" --body-stdin`
3. Add custom sections as needed:
   `echo "body content" | fm section add <stem> "<name>" --body-stdin`
4. Add cross-references to related specs: `fm ref add <stem> <target-stem>`

### Spec Update Workflow

1. Read the current spec: `fm show <stem> --json`
2. Update metadata: `fm update <stem> [--status <s>] [--src <path>] [--purpose "<text>"]`
3. Update section bodies (pipe body via stdin):
   `echo "body content" | fm section set <stem> "<slug>" --body-stdin`

### Commands

#### Core Commands

| Command | Purpose |
|---------|---------|
| `fm create <stem> [--src <path>] --purpose "<text>"` | Create a new spec (scaffolds 5 required sections) |
| `fm show <stem> --json` | Show spec with all sections and refs |
| `fm list [--status <status>] --json` | List all specs, optionally filtered by status |
| `fm update <stem> [--status <s>] [--src <path>] [--purpose "<text>"]` | Update spec metadata |
| `fm delete <stem> [--force]` | Delete a spec (`--force` if sections have content) |
| `fm search "<query>" --json` | Case-insensitive search across stems, purposes, section bodies |
| `fm count [--by-status] --json` | Count specs |
| `fm status --json` | Summary of specs by status |
| `fm history <stem> --json` | Event log for a spec |

#### Section Commands

| Command | Purpose |
|---------|---------|
| `fm section add <stem> "<name>" --body-stdin [--after "<slug>"]` | Add custom section (body from stdin) |
| `fm section set <stem> "<slug>" --body-stdin` | Replace section body (body from stdin) |
| `fm section get <stem> "<slug>" --json` | Get a single section |
| `fm section list <stem> --json` | List all sections for a spec |
| `fm section remove <stem> "<slug>"` | Remove a custom section (required sections are protected) |
| `fm section move <stem> "<slug>" --after "<slug>"` | Reorder a section |

#### Ref Commands

| Command | Purpose |
|---------|---------|
| `fm ref add <stem> <target-stem>` | Add cross-reference (rejects cycles) |
| `fm ref remove <stem> <target-stem>` | Remove cross-reference |
| `fm ref list <stem> --json` | List specs this spec references |
| `fm ref tree <stem> [--direction up\|down] --json` | Recursive ref tree |
| `fm ref cycles --json` | Detect reference cycles |

#### Data & Maintenance Commands

| Command | Purpose |
|---------|---------|
| `fm export` | SQLite → JSONL + generated markdown, stages `.forma/` |
| `fm import` | JSONL → SQLite (used after clone/merge) |
| `fm check --json` | Validation report (required sections, src paths, refs, pensa integration) |
| `fm doctor [--fix] --json` | Health checks; `--fix` removes orphaned data |
| `fm where` | Print JSONL and DB directory paths |


## Voice Output

**Runs if** `$SGF_AGENT` is not set, OR `$SGF_ORCHESTRATOR` is set. Check with: `echo $SGF_AGENT $SGF_ORCHESTRATOR`

Every time you finish responding, call the `run_dic` MCP tool (run in background) to speak a very brief summary aloud. Start with the working directory name, then the summary.

Keep it to a short phrase. Examples:
- "Springfield. Auth refactor done."
- "Dotfiles. Zsh config updated."
- "My project. Need input on migration strategy."
- "Springfield. Stalled, stopping."

Default voice: bf_isabella. A downstream prompt may override the voice.
