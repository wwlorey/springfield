# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning

This document defines backpressure for a variety of project types. Be sure to align your understanding of backpressure to the project type with which you're currently working.

## Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p <crate>` (e.g., `cargo build -p my-crate`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p <crate> <test_name>` (e.g., `cargo test -p my-crate test_login`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Detect unsafe code usage:** `cargo geiger`

### Long Running Tests

Some tests may be gated behind `#[ignore]` because they use expensive operations. These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

### CLI E2E Tests (Tuistory)

Terminal-level end-to-end tests for CLI binaries. Uses [Tuistory](https://github.com/remorses/tuistory) (Playwright for TUIs) driven from cargo integration tests via `std::process::Command`.

- **Run CLI e2e tests:** `cargo test -p <crate> --test cli_e2e`
- **Run single test:** `cargo test -p <crate> --test cli_e2e <test_name>`

## Frontend

> Stack: TypeScript, React, Vitest, Playwright
>
> **Build targets:** Web (`pnpm run build`), Mobile (`pnpm run expo export --platform all`), Tauri (`pnpm run tauri build`)

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

- **E2E tests:** `pnpm run test:e2e`
- **E2E test single file:** `pnpm run test:e2e <path>` (e.g., `pnpm run test:e2e e2e/settings.test.ts`)
- **E2E tests (headed):** `pnpm run test:e2e -- --headed`

