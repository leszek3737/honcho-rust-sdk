# Task Completion Checklist — honcho-ai

After completing a coding task, run these in order:

1. **Format check**: `cargo fmt --check`
2. **Lint**: `cargo clippy --all-targets --all-features`
3. **Fix formatting if needed**: `cargo fmt`
4. **Tests**: `cargo test` (all tests; integration skips if no server)
5. **Docs**: `cargo doc --no-deps` (optional, but ensures doc comments compile)

If only modifying types or DTOs, can run just `cargo test --test '*_types'` for faster feedback.

For integration changes, set `HONCHO_API_URL` and `HONCHO_API_KEY` before running tests.

Retry behavior: HttpClient retries timeout/connection errors and 429|500|502|503|504 responses. Max 2 retries, 500ms initial backoff with exponential delay. Respects Retry-After header.
