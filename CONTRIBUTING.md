# Contributing

## Setup

```bash
cargo check
cargo test
```

## Before opening a PR

```bash
cargo fmt
cargo clippy --lib --bins -- -D warnings -D clippy::dbg_macro -D clippy::todo -D clippy::unwrap_used -D clippy::expect_used -D clippy::unimplemented -D clippy::panic
cargo test
cargo llvm-cov --summary-only --lib --ignore-filename-regex '/target/llvm-cov-target/debug/build/.*/out/' --fail-under-lines 95
```

## Guidelines

- Keep changes focused and small.
- Add/update tests for behavior changes.
- Preserve the interactive UX of the wizard.
- Avoid adding dependencies unless there is a clear payoff.
