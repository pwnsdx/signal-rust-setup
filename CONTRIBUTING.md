# Contributing

## Setup

```bash
cargo check
cargo test
```

## Before opening a PR

```bash
cargo fmt
cargo test
cargo llvm-cov --summary-only --lib --fail-under-lines 95
```

## Guidelines

- Keep changes focused and small.
- Add/update tests for behavior changes.
- Preserve the interactive UX of the wizard.
- Avoid adding dependencies unless there is a clear payoff.
