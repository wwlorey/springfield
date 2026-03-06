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

## Frontend

> Stack: TypeScript, Svelte 5, SvelteKit, Vitest, @testing-library/svelte, Playwright
>
> **Working directory:** adjust as needed (some projects may have frontend commands run from the frontend directory)

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

### E2E Tests (Playwright)

- **E2E tests:** `pnpm run test:e2e`

### E2E Tests (Tauri, Linux Only)

E2E tests run on **Linux only** using WebdriverIO + WebKitWebDriver. macOS is not supported for E2E testing (no WebDriver access to WKWebView).

## Tauri

- **Build Tauri app:** `pnpm run tauri build`
- **Build Tauri app (debug):** `pnpm run tauri build --debug`

## Mobile (React Native + Expo)

> **Working directory:** repo root

| Layer | Tool | Notes |
|---|---|---|
| Language | TypeScript (strict mode) | `strict` + extended flags enforced via `tsconfig.json` |
| Framework | React Native + Expo | Cross-platform iOS + Android |
| State management | Zustand | Simple, typed, minimal boilerplate |
| Local storage | MMKV | Fast, typed — swap for Supabase when backend needed |
| Backend (future) | Supabase | TypeScript SDK, auth, sync |
| Package manager | Yarn | Standard for React Native ecosystem |
| Unit & component tests | Jest + jest-expo | Via `@testing-library/react-native` for components |
| E2E tests | Maestro | Simulator/emulator only, YAML flows, Mac + Linux |
| Type coverage | type-coverage | Enforces ≥95% typed symbols |
| Dead code detection | knip | Unused exports, files, and dependencies |
| Circular import detection | madge | Catches circular import chains |
| Linting | ESLint + @typescript-eslint/strict | Type-aware lint rules |
| Accessibility lint | eslint-plugin-react-native-a11y | Enforces a11y props on all components |
| Formatting | Prettier | Code style enforcement |
| Secrets scanning | gitleaks | Detects hardcoded credentials |
| Vulnerability audit | yarn audit | Fails on high/critical CVEs |
| Duplicate dependencies | yarn dedupe | Prevents duplicate package versions |
| Build validation | expo export | Full Metro bundler build check |
| Env var validation | validate:env script | Fails if required env vars are missing |

### Stack Reference

This table defines the canonical stack for this project.

| Layer | Choice | Rationale |
|---|---|---|
| Framework | React Native + Expo | Agent-friendly, fast, cross-platform |
| Language | TypeScript (strict mode) | Hard typed, best agent codegen |
| e2e Testing | Maestro | Mac + Linux, simulator support, YAML = agent-friendly |
| CI/CD | GitHub Actions | Mac + Linux runners, widely supported |
| State | Zustand | Simple, typed, low boilerplate |
| Local Storage | MMKV | Fast, typed, easy to swap for Supabase later |
| Backend (later) | Supabase | TypeScript SDK, drop-in sync, auth included |

### Type Safety

- **Validate tsconfig strict flags:** `node -e "const t=require('./tsconfig.json'),o=t.compilerOptions,r=['strict','noUncheckedIndexedAccess','exactOptionalPropertyTypes','noImplicitReturns','noFallthroughCasesInSwitch'],m=r.filter(k=>!o[k]);if(m.length){console.error('Missing required tsconfig flags:',m);process.exit(1)}else{console.log('tsconfig OK')}"`
- **Type check:** `yarn tsc --noEmit`
- **Type coverage (must be ≥95%):** `yarn type-coverage --atLeast 95 --strict`
- **Detect unused code and dependencies:** `yarn knip`
- **Detect circular imports:** `yarn madge --circular --extensions ts,tsx src/`

### Build Validation

- **Full Metro bundler build (all platforms):** `yarn expo export --platform all`

### Security & Health

- **Scan for secrets and credentials:** `gitleaks detect --source .`
- **Expo dependency and config health check:** `yarn expo doctor`
- **Dependency vulnerability audit:** `yarn audit --level high`
- **Detect duplicate dependencies:** `yarn dedupe --check`
- **Validate required environment variables:** `yarn validate:env`

> `validate:env` must be implemented as a script in `package.json` that checks all required `process.env` variables are present and non-empty, exiting non-zero if any are missing. Agents adding new `process.env` references must update this script.

### Linting & Formatting

- **Lint (style + type-aware):** `yarn lint`
- **Lint fix:** `yarn lint:fix`
- **Accessibility lint:** `yarn lint:a11y`
- **Format:** `yarn format`
- **Format check:** `yarn format:check`

> `lint:a11y` must run ESLint with `eslint-plugin-react-native-a11y` enabled. Agents must never remove or disable accessibility rules.

### Unit Tests

- **Unit tests:** `yarn test --passWithNoTests`
- **Unit test single file:** `yarn test <path>` (e.g., `yarn test src/components/TodoItem.test.tsx`)

### Component Tests

Component tests use `@testing-library/react-native` to render components in isolation and assert on their output without a running simulator. These are faster than e2e tests and should cover all non-trivial UI components.

- **Component tests:** `yarn test --testPathPattern="src/components" --passWithNoTests`
- **Component test single file:** `yarn test <path>` (e.g., `yarn test src/components/TodoItem.test.tsx`)

### E2E Tests (Maestro)

E2E tests run against simulators and emulators only — no real devices required. An iOS Simulator (macOS only) or Android Emulator (macOS and Linux) must be booted before running these commands. Maestro auto-detects the running simulator.

- **E2E all flows (iOS):** `maestro test e2e/` (run with iOS Simulator active)
- **E2E all flows (Android):** `maestro test e2e/` (run with Android Emulator active)
- **E2E single flow:** `maestro test e2e/<flow>.yaml` (e.g., `maestro test e2e/create_todo.yaml`)

> **Platform note:** iOS Simulator requires macOS. Android Emulator runs on both macOS and Linux. On Linux CI, only Android e2e tests are executable.

