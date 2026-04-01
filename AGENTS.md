Rust workspace for orchestrating AI-driven dev via iterative agent loops. CLI: `sgf`. Codifies the Ralph Wiggum technique.

## Project Structure

- Rust workspace with crates under crates/.
- Specs and implementation plans under specs/.

## Installing

```
just install
```

## Code Style

- **Async:** Tokio runtime. Use `async-trait` for async trait methods.
- **No comments** unless code is complex and requires context for future developers.
- **Logging:** Use structured logging (`tracing`). Never log secrets directly.
- **Instrumentation:** Use `#[instrument(skip(self, secrets, large_args), fields(id = %id))]`. Always skip secrets.
- **Process spawns:** `Child` has no `Drop` — dropped handles leak processes. Use `shutdown::ChildGuard` for `.spawn()` calls; never fire-and-forget a spawn. In tests, use `shutdown::ProcessSemaphore` to throttle `.output()` calls and set `SGF_TEST_NO_SETSID=1` on spawned commands so children stay killable.
- **TypeScript scripts:** Never use bare `tsx` as a script runner in `package.json` — the Claude Code sandbox blocks the IPC pipe it spawns. Use `node --import tsx/esm <script>` instead.
- When running Playwright tests:
  + Playwright should be configured with `workers: 1` to avoid macOS Chromium sandbox limits.
  + Chromium may fail with `bootstrap_check_in ... Permission denied` due to the Claude Code sandbox. The `chromiumSandbox: false` setting in `playwright.config.ts` works around this — do not remove it.

## IMPORTANT

- **ALWAYS read the given prompt files at the beginning of each session.**
- Specs are the SOURCE OF TRUTH.
- Do not update ./.sgf/BACKPRESSURE.md or ./.sgf/MEMENTO.md without EXPLICIT approval.
