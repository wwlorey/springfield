## Project Structure

- Rust workspace with crates under crates/.
- Specs and implementation plans under specs/.

## Backpressure (**Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning**)

### Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p buddy-<crate>` (e.g., `cargo build -p buddy-llm`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p buddy-<crate> <test_name>` (e.g., `cargo test -p buddy-llm test_agent`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Detect unsafe code usage:** `cargo geiger`


## Code Style

- **Async:** Tokio runtime. Use `async-trait` for async trait methods.
- **No comments** unless code is complex and requires context for future developers.
- **Logging:** Use structured logging (`tracing`). Never log secrets directly.
- **Instrumentation:** Use `#[instrument(skip(self, secrets, large_args), fields(id = %id))]`. Always skip secrets.


