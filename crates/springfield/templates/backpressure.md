# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning

After making changes, apply FULL BACKPRESSURE to verify behavior.

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

Some tests are gated behind `#[ignore]` because they use expensive operations (e.g., production Argon2 params, real LLM inference). These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

### Model-Dependent Tests (Requires Downloaded Models)

Some ignored tests require large models to be present. The model may be auto-downloaded on first build.

```bash
cargo test -p <crate> -- --ignored --test-threads=1
```

These tests run LLM inference on CPU and are slow (~2-10 min per test). Use `--test-threads=1` to avoid memory exhaustion from multiple model instances.

---

## Frontend (Tauri, SvelteKit)

> Stack: TypeScript, Svelte 5, SvelteKit (static adapter), Vitest, @testing-library/svelte, WebdriverIO
>
> **Working directory:** adjust as needed (all frontend commands run from the frontend directory)

- **Build frontend:** `pnpm build`
- **Build Tauri app:** `pnpm tauri build`
- **Build Tauri app (debug):** `pnpm tauri build --debug`
- **Unit tests:** `pnpm vitest run`
- **Unit tests (watch):** `pnpm vitest`
- **Unit test single file:** `pnpm vitest run <path>` (e.g., `pnpm vitest run src/lib/components/Auth/LoginScreen.test.ts`)
- **Type check:** `pnpm tsc --noEmit`
- **Svelte check:** `pnpm svelte-check --tsconfig ./tsconfig.json`
- **Lint:** `pnpm lint`
- **Lint fix:** `pnpm lint:fix`
- **Format:** `pnpm format`
- **Format check:** `pnpm format:check`

### E2E Tests (Linux Only)

E2E tests run on **Linux only** using WebKitWebDriver. macOS is not supported for E2E testing (no WebDriver access to WKWebView).

**Linux prerequisites:**
```bash
sudo apt-get install webkit2gtk-driver libwebkit2gtk-4.1-dev
```

**Running E2E tests:**
- **E2E tests (debug build, default):** `BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js`
- **E2E tests (release build):** `WDIO_RELEASE=1 BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js`
- **E2E single test:** `BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js --spec e2e/auth.test.ts`

**Environment variables:**
- `BUDDY_MOCK_AUDIO=1` - Required for recording tests (uses mock audio file instead of real microphone)
- `BUDDY_MOCK_LLM=1` - Use canned LLM responses (fast, for CI)
- `BUDDY_E2E_ISOLATED=1` - Clear app data before test suite (for full isolation)
- `WDIO_RELEASE=1` - Use release build instead of debug (default is debug for faster iteration)

---

## Frontend (SvelteKit, Vite)

> Stack: JavaScript, Svelte, Vitest, Playwright

- **Build:** `pnpm run build`
- **Unit tests:** `pnpm run test`
- **Unit tests (watch):** `pnpm run test:watch`
- **Unit test single file:** `pnpm vitest run <path>` (e.g., `pnpm vitest run src/lib/stores/progress.test.js`)
- **E2E tests:** `pnpm run test:e2e`
- **Lint:** `pnpm run lint`
- **Lint fix:** `pnpm run lint:fix`
- **Format:** `pnpm run format`
- **Format check:** `pnpm run format:check`
- **Validate data:** `pnpm run validate:data`
- **Full check:** `pnpm run check`
