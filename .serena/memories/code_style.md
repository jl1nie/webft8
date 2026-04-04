# Code Style and Conventions

## General
- Rust edition 2024
- No `unsafe` (all safe Rust)
- No `unwrap()` in library code — use `?` or explicit match; `unwrap()` only in tests
- No `println!` in ft8-core (library); ft8-bench may use it for reports

## Naming
- Functions: `snake_case`
- Types/structs/enums: `PascalCase`
- Constants: `SCREAMING_SNAKE_CASE`
- Module files match their `pub mod` name

## Comments
- Module-level doc comment at top of each file explaining purpose and algorithm source
- Inline comments only where logic is non-obvious
- Section separators: `// ──── Section Name ─────` (80 chars)
- Algorithm references cite WSJT-X source file and line numbers (e.g. `// ported from ft8b.f90 lines 154-239`)

## Types
- Audio input: `&[i16]` (16-bit PCM at 12000Hz)
- Complex: `num_complex::Complex<f32>`
- FFT: `rustfft::FftPlanner<f32>`
- Fixed-size LDPC arrays: `[f32; 174]`, `[u8; 77]` etc. (stack/heap via Box)

## Testing
- Unit tests in `#[cfg(test)] mod tests` at bottom of each file
- Test names describe the scenario, not the function (e.g. `silence_no_decode`)
- Property-based: test boundary conditions (silence, zero input, known-good input)
- 18 tests in Phase 1, all must pass before commit

## Error handling
- `bp_decode` returns `Option<BpResult>` (None = decode failure)
- `decode_frame` returns `Vec<DecodeResult>` (empty = nothing decoded)
- No panics in hot paths

## Stub files
- Unimplemented modules contain `// TODO: implement` with a brief description
- Must compile without warnings
