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

## IMPORTANT

- **ALWAYS read the given prompt files at the beginning of each session.**
- Do not update ./.sgf/BACKPRESSURE.md or ./.sgf/MEMENTO.md without EXPLICIT approval.
