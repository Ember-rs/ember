# Contributing

Scafra is a modular Rust workspace. Contributions are welcome in code,
documentation, tests, examples, diagnostics, and generated starter templates.

## Before opening a change

Read the [architecture guide](architecture.md) and identify the smallest crate
that owns the behavior. Prefer a focused change with a test or documentation
example that demonstrates the intended behavior.

```mermaid
flowchart LR
    A[Choose a focused crate] --> B[Add or update a test]
    B --> C[Run workspace checks]
    C --> D[Update docs or examples]
    D --> E[Open a focused change]
```

## Local checks

Run the checks that match your change, and preferably the full workspace suite:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

For CLI or generated-project changes, also run the relevant `scafra new`,
`scafra check`, or generated-project integration tests.

## Contribution principles

- Prefer ordinary Rust types and compiler-checked behavior.
- Keep generated code readable and inspectable.
- Avoid adding runtime reflection, global mutable containers, or string-key
  dependency lookup as the primary architecture.
- Keep public APIs small and document the user-facing behavior.
- Add regression tests for diagnostics, configuration precedence, generated
  files, and lifecycle behavior when those areas change.
