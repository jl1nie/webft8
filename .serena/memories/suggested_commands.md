# Suggested Commands

## Build
```bash
cargo build                          # build all crates
cargo build -p ft8-core              # library only
cargo build --target wasm32-unknown-unknown -p ft8-core  # WASM check
```

## Test
```bash
cargo test                           # all tests in workspace
cargo test -p ft8-core               # library tests only
cargo test -p ft8-core -- --nocapture  # with stdout
cargo test <test_name>               # single test
```

## Lint / format
```bash
cargo clippy -- -D warnings          # lint (treat warnings as errors)
cargo fmt                            # auto-format
cargo fmt --check                    # check formatting without changing files
```

## Run bench
```bash
cargo run -p ft8-bench               # run test bench (Phase 2+)
cargo run -p ft8-bench -- --help     # help (once implemented)
```

## Check WASM compatibility
```bash
cargo check --target wasm32-unknown-unknown -p ft8-core
```

## Git
```bash
git log --oneline -10
git diff HEAD
git add ft8-core/src/<file> && git commit -m "..."
```
