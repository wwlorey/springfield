# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning

**After making changes, apply FULL BACKPRESSURE to verify behavior as appropriate.**

---

## Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p <crate>` (e.g., `cargo build -p my-crate`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p <crate> <test_name>` (e.g., `cargo test -p my-crate test_login`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Detect unsafe code usage:** `cargo geiger`

### Long Running Tests

Some tests may be gated behind `#[ignore]` because they use expensive operations (e.g., production Argon2 params, real LLM inference). These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

---

## Frontend

> Stack: TypeScript, Svelte 5, SvelteKit, Vitest, @testing-library/svelte, Playwright
>
> **Working directory:** adjust as needed (all frontend commands run from the frontend directory, as applicable)

- **Build:** `pnpm run build`
- **Unit tests:** `pnpm run vitest run`
- **Unit tests (watch):** `pnpm run vitest`
- **Unit test single file:** `pnpm run vitest run <path>` (e.g., `pnpm run vitest run src/lib/components/Auth/LoginScreen.test.ts`)
- **Type check:** `pnpm run tsc --noEmit`
- **Svelte check:** `pnpm run svelte-check --tsconfig ./tsconfig.json`
- **Lint:** `pnpm run lint`
- **Lint fix:** `pnpm run lint:fix`
- **Format:** `pnpm run format`
- **Format check:** `pnpm run format:check`
- **Full check:** `pnpm run check`

### Tauri

Delete this section if the project does not use Tauri.

- **Build Tauri app:** `pnpm run tauri build`
- **Build Tauri app (debug):** `pnpm run tauri build --debug`

### E2E Tests (Playwright)

Delete this section if the project uses Tauri (use the Tauri E2E section below instead).

- **E2E tests:** `pnpm run test:e2e`

### E2E Tests (Tauri, Linux Only)

Delete this section if the project does not use Tauri.

E2E tests run on **Linux only** using WebdriverIO + WebKitWebDriver. macOS is not supported for E2E testing (no WebDriver access to WKWebView).

**Linux prerequisites:**
```bash
sudo apt-get install webkit2gtk-driver libwebkit2gtk-4.1-dev
```
