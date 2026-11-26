## Build/test
- Needs Rust >= 1.85 (Cargo.lock v4). `cargo test --bin catan-lan`
- Tests need web/scripts/board-golden.json
- Known: game.rs test helper random_settings missing `speed` field

## Conventions
- Wire changes: update docs/Protocol.md and web/src/types.ts
- Deliver only changed files, same paths