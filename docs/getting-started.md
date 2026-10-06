# Getting started

## Requirements

- Rust 1.82 or newer
- Cargo

The CLI is currently run from this workspace.

## Generate an application

From the Ember workspace:

```bash
cargo run -p ember-cli -- new hello-ember
cd hello-ember
cargo check
cargo run
```

The default starter is a web application. Other supported shapes are:

```bash
cargo run -p ember-cli -- new hello-api --kind api
cargo run -p ember-cli -- new hello-service --kind service
cargo run -p ember-cli -- new hello-monolith --kind monolith
```

The generated server listens on `127.0.0.1:8080` by default. Configuration can
be placed in `src/resources/application.yaml` or
`src/resources/application.properties`.

## Run the checks

From the Ember workspace:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-features
```

## Use the generated application

Generated projects use the `ember` facade and typed component macros. A small
application can define a service and controller without a runtime service
locator:

```rust
use ember::prelude::*;

#[service]
struct GreetingService;

#[controller("/api")]
struct GreetingController {
    service: GreetingService,
}

#[routes]
impl GreetingController {
    #[get("/hello/{name}")]
    async fn hello(&self, name: Path<String>) -> String {
        format!("Hello, {}!", name)
    }
}

#[ember::main]
async fn main() {}
```

See the [generated-code guide](generated-code.md) for the project layout and
the [architecture guide](architecture.md) for the runtime flow.
