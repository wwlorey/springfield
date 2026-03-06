Rust workspace for orchestrating AI-driven dev via iterative agent loops. CLI: `sgf`. Codifies the Ralph Wiggum technique.

## Project Structure

- Rust workspace with crates under crates/.
- Specs and implementation plans under specs/.

## Installing

```
cargo install --path crates/pensa
cargo install --path crates/ralph
cargo install --path crates/springfield
```

## Code Style

- **Async:** Tokio runtime. Use `async-trait` for async trait methods.
- **No comments** unless code is complex and requires context for future developers.
- **Logging:** Use structured logging (`tracing`). Never log secrets directly.
- **Instrumentation:** Use `#[instrument(skip(self, secrets, large_args), fields(id = %id))]`. Always skip secrets.

## IMPORTANT

- Always read the given prompt files at the beginning of each session.
