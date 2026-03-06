# Pensa daemon sharing via sandbox proxy

## Problem

Inside Docker sandboxes, `pn` can't reach the host daemon because `no_proxy=localhost` prevents the HTTP proxy from routing localhost traffic. So `pn` auto-starts a local daemon with a stale/empty DB. This causes `pn ready --json` to return `[]` inside sandboxes, making `sgf build` think there's no work and exit immediately.

## Solution

Ralph writes a `.pensa/daemon.url` file before launching the sandbox. `pn` reads it, detects it's behind a proxy, and forces traffic through the proxy (ignoring `no_proxy`). Ralph also configures the sandbox network policy to allow `localhost` through the proxy.

Key discovery: the sandbox's HTTP proxy at `host.docker.internal:3128` CAN route `localhost` requests back to the host — but only when (1) `localhost` is added to the proxy allow list via `docker sandbox network proxy <name> --allow-host localhost`, and (2) the `no_proxy` env var is bypassed so reqwest actually uses the proxy for localhost URLs.

Verified working:
```
docker sandbox network proxy claude-springfield --allow-host localhost
docker sandbox exec claude-springfield bash -c \
  'no_proxy="" NO_PROXY="" curl -s --noproxy "" \
   --proxy http://host.docker.internal:3128 \
   http://localhost:13248/status'
# Returns host daemon data
```

Also verified: `PN_DAEMON=http://localhost:13248 pn ready --json` returns correct data when the proxy routes correctly.

---

## 1. `crates/pensa/src/client.rs` — Read `daemon.url`, force proxy

- In `Client::new()`, add `.pensa/daemon.url` to the resolution order. Current order: (1) `PN_DAEMON_HOST`, (2) `PN_DAEMON`, (3) localhost fallback. New order: (1) `PN_DAEMON_HOST`, (2) `PN_DAEMON`, (3) **`.pensa/daemon.url`**, (4) localhost fallback. Read from `<cwd>/.pensa/daemon.url`. If the file exists and contains a non-empty trimmed URL, use it as the base URL.
- When the daemon URL comes from `daemon.url` or `PN_DAEMON`: if `http_proxy` or `HTTP_PROXY` env var is set, build the reqwest client with an explicit `Proxy::all(<proxy_url>)` and `NoProxy::none()` (empty no_proxy list). This forces localhost traffic through the proxy. Otherwise, build the default client.
- Tests:
  - Unit test that `daemon.url` is read and used as base URL when present.
  - Unit test that `PN_DAEMON` takes priority over `daemon.url`.
  - Unit test that when `HTTP_PROXY` is set + URL comes from `daemon.url`, the client is built with forced proxy config (test the builder logic, not actual HTTP).
  - Unit test that normal localhost fallback still works when no `daemon.url` exists.

## 2. `crates/pensa/src/main.rs` — Treat `daemon.url` as remote

- Update `is_remote_host()` to also return `true` when `<cwd>/.pensa/daemon.url` exists and contains a non-empty URL. This prevents `ensure_daemon()` from auto-starting a local daemon inside the sandbox.
- Tests:
  - Unit test that `is_remote_host()` returns `true` when `daemon.url` exists with content.
  - Unit test that `is_remote_host()` returns `false` when `daemon.url` does not exist (and no env vars set).

## 3. `crates/ralph/src/main.rs` — Write `daemon.url`, configure proxy

- Add `write_daemon_url()`: reads `.pensa/daemon.port` from cwd, writes `.pensa/daemon.url` containing `http://localhost:<port>`. No-op with warning if `daemon.port` doesn't exist or can't be parsed.
- Add `remove_daemon_url()`: deletes `.pensa/daemon.url` if it exists. Called on all exit paths so the file doesn't persist on the host and confuse host-side `pn`.
- Add `configure_sandbox_network()`: runs `docker sandbox network proxy <sandbox-name> --allow-host localhost`. Fire-and-forget (log warning on failure). The sandbox name is `claude-<workspace_dir_basename>` (matching Docker sandbox's default naming: `<agent>-<workdir>`). Use `docker_command()` for context handling.
- Call `write_daemon_url()` in `main()` after `ensure_sandbox()`, before the iteration loop.
- Call `configure_sandbox_network()` after `ensure_sandbox()`.
- Call `remove_daemon_url()` on all exit paths (normal completion, max iterations, interrupt). Consider using a Drop guard or calling at each exit point.
- Skip `write_daemon_url()` and `configure_sandbox_network()` when `--command` is set (test mode — no sandbox).
- Tests:
  - Unit test that `write_daemon_url()` reads `daemon.port` and writes correct URL.
  - Unit test that `remove_daemon_url()` cleans up the file.
  - Integration test with `--command` mock verifying `daemon.url` does NOT exist (test mode skips it).

## 4. Dockerfile — Remove `PN_DAEMON_HOST` override

- In the embedded Dockerfile (find it via `grep -r "PN_DAEMON_HOST" crates/springfield/`): remove the `ENV PN_DAEMON_HOST=` line and its comment block. This env var override was the old approach (clear the base image's PN_DAEMON_HOST so pn auto-starts locally). No longer needed since pn will use `daemon.url` instead.
- The template hash will change automatically, triggering a rebuild on next `sgf build`.

## 5. Gitignore — Add `daemon.url`

- Add `**/.pensa/daemon.url` to the project `.gitignore` (alongside existing `**/.pensa/db.sqlite` and `**/.pensa/daemon.port`).
- Update `crates/springfield/src/init.rs` gitignore template entries to include `.pensa/daemon.url` so future `sgf init` projects get it.

## 6. Spec updates

### `specs/pensa.md`

- Rewrite the "Docker sandbox strategy" paragraph (currently line 48). Replace:
  > Docker sandbox network policy blocks connections from the container back to the host, making `host.docker.internal` unreachable. Instead, `pn` auto-starts a local daemon inside the sandbox that reads/writes the Mutagen-synced `.pensa/db.sqlite`. The sandbox template clears `PN_DAEMON_HOST` (set by the base image) so the local auto-start path is taken. On first launch, the daemon auto-imports from the JSONL files synced into the workspace. The host runs `pn export` before launching the sandbox to ensure JSONL is current.

  With new text describing: Ralph writes `.pensa/daemon.url` containing `http://localhost:<port>` before sandbox launch. When `pn` finds this file, it treats the daemon as remote (no auto-start) and forces HTTP traffic through the sandbox's HTTP proxy (ignoring `no_proxy=localhost`). The proxy routes `localhost` requests back to the host, where the real daemon is running. Ralph configures the sandbox network policy to allow `localhost` through the proxy via `docker sandbox network proxy <name> --allow-host localhost`. This ensures all sandbox instances share the same daemon and database — no stale copies, atomic claims work across sandboxes.

- Add `daemon.url` mention in the "Storage Model" section as a transient, gitignored file written by ralph.

- Update the CLI client resolution order documentation (line 46) to include `daemon.url` in the resolution chain.

### `specs/ralph.md`

- Add a new section "Sandbox Pensa Configuration" (after "System Prompt Injection" or before "Modes") describing:
  - Ralph writes `.pensa/daemon.url` before launching Claude (reads `daemon.port`, writes `http://localhost:<port>`)
  - Ralph runs `docker sandbox network proxy <name> --allow-host localhost`
  - Ralph cleans up `daemon.url` on exit
  - Both are skipped when `--command` is set (test mode)

### `specs/springfield.md`

- Update "Sandbox Behavior > Pensa access" (line 777). Replace the current paragraph about clearing `PN_DAEMON_HOST` and auto-starting local daemons with the new proxy-based approach: ralph writes `.pensa/daemon.url`, configures sandbox network, `pn` connects to host daemon through the proxy.
- Update the embedded Dockerfile to remove the `ENV PN_DAEMON_HOST=` line and its comment (lines 740-743).
- Add `.pensa/daemon.url` to the gitignore entries section (around line 126).
