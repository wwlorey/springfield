# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Coverage

This document defines backpressure for a variety of project types. Be sure to align your understanding of backpressure to the project type with which you're currently working.

## Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p <crate>` (e.g., `cargo build -p my-crate`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p <crate> <test_name>` (e.g., `cargo test -p my-crate test_login`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Code coverage:** `cargo llvm-cov --workspace`
- **Code coverage (single crate):** `cargo llvm-cov -p <crate>`

### Long Running Tests

Some tests may be gated behind `#[ignore]` because they use expensive operations. These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

### CLI E2E Tests (Tuistory)

Terminal-level end-to-end tests for CLI binaries. Uses [Tuistory](https://github.com/remorses/tuistory) (Playwright for TUIs) driven from cargo integration tests via `std::process::Command`.

- **Run CLI e2e tests:** `cargo test -p <crate> --test cli_e2e`
- **Run single test:** `cargo test -p <crate> --test cli_e2e <test_name>`

### Tauri App (MCP)

 These commands require windowing/GPU access and MUST use `unsandboxed-runner` MCP tools — not Bash.

- **Smoke test:** `smoke_test_tauri` with `cwd` (e.g. `cwd: "crates/lsr-app"`) (validates the app boots without runtime panics — run after backend changes)
- **Build:** `run_tauri_build` with `cwd` (e.g. `cwd: "crates/lsr-app"`)

## Frontend

> Stack: TypeScript, React 19, Vite, Zustand, shadcn/ui, Tailwind CSS v4, Vitest, @testing-library/react, Playwright
>
> **Build targets:** Web (`pnpm run build`), Mobile (`pnpm run expo export --platform all`), Tauri (`run_tauri_build`)

- **Build:** `pnpm run build`
- **Unit tests:** `pnpm run test:unit`
- **Unit test single file:** `pnpm run test:unit <path>` (e.g., `pnpm run test:unit src/components/Auth/LoginScreen.test.tsx`)
- **Type check:** `pnpm run typecheck` (should be configured to run at least `pnpm run tsc --noEmit`)
- **Lint:** `pnpm run lint`
- **Lint fix:** `pnpm run lint:fix`
- **Format:** `pnpm run format`
- **Format check:** `pnpm run format:check`

### E2E Tests (Playwright)

E2E tests run against the dev server (web/Tauri) or web export (mobile) with a mocked backend. No native binary, simulator, or backend build required.

- **E2E tests:** `run_playwright` MCP tool (do NOT use Bash — sandbox blocks Chromium)
- **E2E test single file:** `run_playwright` with `file: "<path>"` param (e.g., `file: "e2e/settings.test.ts"`)

These MCP tools accept optional `cwd` (relative to project root) and `timeout_secs` (default 300, max 600).

### Component Stories (Ladle)

Component isolation and visual development environment. Renders real React components outside of Tauri — fast reload, no backend required. Stories are co-located with components as `*.stories.tsx` files.

- **Dev server:** `pnpm run storybook`
- **Build (CI):** `pnpm run storybook:build` (include in full backpressure — catches broken stories)
