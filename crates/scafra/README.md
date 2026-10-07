# Scafra framework

The `scafra` package provides Scafra's application-facing Rust
framework. Add it under the dependency key `scafra` to keep the familiar crate
name in source code:

```toml
[dependencies]
scafra = "0.1"
```

```rust
use scafra::prelude::*;
```

Scafra is built on Axum, Tokio, Tower, Serde, and `tracing`. It provides typed
component and route macros, configuration loading, application startup, and
optional operational features. See the [repository README](https://github.com/Scafra/Scafra)
for examples, supported behavior, and current limitations.
