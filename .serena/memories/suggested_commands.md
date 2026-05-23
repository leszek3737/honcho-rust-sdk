# Suggested Commands — honcho-ai (Rust SDK)

## Build
```bash
cargo build
cargo build --all-features
```

## Testing
```bash
cargo test                              # all tests (unit + integration; int tests skip if no server)
cargo test --lib                        # unit tests only
cargo test --test '*_types'             # schema validation + roundtrip tests (no server needed)
cargo test --test integration           # integration tests (needs HONCHO_API_URL and HONCHO_API_KEY env vars)
```

## Lint & Format (run in this order)
```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo fmt
```

## Docs
```bash
cargo doc --no-deps
cargo doc --no-deps --open
```

## Pre-commit checklist
Run in order: `cargo fmt --check && cargo clippy --all-targets --all-features && cargo test`

## Integration tests env
```bash
export HONCHO_API_URL=http://localhost:8000
export HONCHO_API_KEY=your-key
```

## Git
Standard git commands. Use conventional commits.
