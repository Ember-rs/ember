# Generated projects

The `scafra new` command creates ordinary, inspectable Rust files. It does not
generate a hidden application bundle or require a special editor.

## Typical layout

```text
my-app/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs
    ├── main/
    │   ├── controllers/
    │   ├── services/
    │   ├── repositories/
    │   ├── beans/
    │   ├── config/
    │   ├── models/
    │   └── errors/
    └── resources/
        └── application.yaml
```

The monolith starter additionally creates a bounded module under
`src/main/modules/`.

## Source discovery

The generated `#[scafra::main]` macro discovers Rust files below `src/main/`
during compilation and emits the module declarations needed by the application.
New Rust files under `src/main/` can therefore participate without manually
adding every `mod` declaration to `main.rs`.

The same macro generates the typed dependency graph from these source files.
During standard startup, graph providers are constructed in dependency order
and controller instances are registered with their routes. An application can
also add an application-owned `build.rs` and call
`scafra_build::discover_graph("src/main")` when it needs the generated graph
artifact in a custom build pipeline.

The generated files remain normal Rust code. Visibility, imports, compiler
diagnostics, and application ownership are still explicit.

## From source file to route

```mermaid
sequenceDiagram
    participant Dev as Developer
    participant Cargo
    participant Scafra as Scafra macros
    participant App as Application
    participant Server as Axum/Tokio

    Dev->>Cargo: cargo run
    Cargo->>Scafra: Expand #[scafra::main]
    Scafra->>Scafra: Discover src/main/
    Cargo->>Scafra: Expand typed components and routes
    Scafra-->>App: Compile modules and registrations
    App->>Server: Build router and start server
```

## Customization

The starter is a convention, not a lock-in. Replace controllers, services,
providers, configuration, or the entry point as the application evolves. If
the standard runner is replaced, the application must include the generated
module file or declare equivalent modules manually so static registrations stay
linked.
