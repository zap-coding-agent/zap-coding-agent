---
category: domain
name: rust
trigger: ["rust", "cargo", "crate", "fn ", "struct ", "enum ", "impl ", "trait ", "tokio", "async fn", "clippy", "rustfmt", ".rs"]
tokens: ~700
---

## Rust conventions

**Error handling:** Use `anyhow::Result` for application code. Use `thiserror` for library error types. Always propagate with `?`, never `.unwrap()` in production paths. Add `.context("what failed")` to give errors meaning.

**Ownership:** Prefer borrowing over cloning. Clone only at system boundaries. Use `Arc` for shared ownership across threads, `Rc` only in single-threaded contexts.

**Async:** Use `tokio` runtime. Mark functions `async fn` only when they actually await. Don't block the async runtime with `std::thread::sleep` or heavy CPU work — use `tokio::task::spawn_blocking` for that.

**Style:** Follow `rustfmt` defaults. Use `clippy` before committing. Prefer `if let` over `match` for single-arm patterns. Use `?` operator over explicit `match Err(e) => return Err(e)`.

**Naming:** `snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants. Avoid abbreviations except well-known ones (`cfg`, `buf`, `ctx`).

**Modules:** Keep `mod.rs` files thin — they declare sub-modules and re-export public items. Implementations go in named files. Use `pub(crate)` to limit visibility to the crate.

**Testing:** Unit tests go in a `#[cfg(test)]` module at the bottom of the file. Integration tests go in `tests/`. Use `#[tokio::test]` for async tests.

**Cargo:** Keep `Cargo.toml` dependencies minimal. Pin major versions. Prefer `{ version = "1", features = ["..."] }` over `"*"`.
