# Task Completion Checklist

After completing any coding task in this project:

1. **Run tests** — `cargo test` must pass with 0 failures
2. **Check clippy** — `cargo clippy -- -D warnings` must produce no errors
3. **Format** — `cargo fmt --check` (or run `cargo fmt` first)
4. **WASM check** (for ft8-core changes) — `cargo check --target wasm32-unknown-unknown -p ft8-core`
5. **Commit** — stage specific files, write a descriptive commit message

## Commit message style
```
feat: short description of what was added
fix: short description of what was fixed
refactor: ...
test: ...
```
Body: explain *why* if non-obvious. End with:
```
Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
```
