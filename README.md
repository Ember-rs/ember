# Ember

Ember is an open-source, batteries-included web application framework for
Rust. It gives common applications a structured composition root while keeping
Axum, Tokio, Tower, Serde, and tracing visible at the edges.

The project is an MVP. It currently focuses on one complete path and a first
shape-aware project generator:

~~~text
typed components -> generated routes -> Axum router -> Tokio server
~~~

The starter generator currently supports web applications, JSON APIs,
services, and modular monoliths. The starters use the same Ember programming
model; the selected shape changes the sample code and layout, not the runtime
architecture.

The product rule for Ember is broader than this first slice: Ember should
provide the default application infrastructure developers need for web
applications, APIs, monoliths, and microservices out of the box. The roadmap
therefore treats the current HTTP slice as the foundation, not the final
framework boundary.

## Quick start from this workspace

~~~bash
cargo run -p hello-world
curl http://127.0.0.1:8080/api/hello/Alice
# Hello, Alice!
~~~

The same application shape is available to users through the facade crate:

~~~rust
use ember::prelude::*;

#[service]
struct GreetingService;

impl GreetingService {
    fn greet(&self, name: &str) -> String {
        format!("Hello, {name}!")
    }
}

#[controller("/api")]
struct GreetingController {
    service: GreetingService,
}

#[routes]
impl GreetingController {
    #[get("/hello/{name}")]
    async fn hello(&self, name: Path<String>) -> String {
        self.service.greet(&name)
    }
}

#[ember::main]
async fn main() {}
~~~

'#[service]' generates a typed constructor and a 'Default' implementation that
constructs fields with 'Default::default()'. A missing dependency constructor
is therefore a normal compile error. '#[bean]' turns a concrete provider
function into the typed default for its return type, so services can depend on
beans without looking them up by string. '#[routes]' generates ordinary Axum
handler adapters. The default path uses link-time static route descriptors to
support the empty application entry point; it does not provide a runtime
service locator or reflection-based scanning. Applications that need explicit
multi-file provider wiring can opt into the build-time typed graph with
`ember_build::discover_graph`; see the [typed graph guide](docs/guides/dependency-graph.md).

## Create an application

From this checkout, create a starter with `ember new`:

~~~bash
cargo run -p ember-cli -- new storefront
cargo run -p ember-cli -- new billing-api --kind api
cargo run -p ember-cli -- new catalog --kind service
cargo run -p ember-cli -- new admin --kind web
cargo run -p ember-cli -- new backoffice --kind monolith
~~~

Omitting `--kind` defaults to `web`. The canonical values are `web`, `api`,
`service`, and `monolith`; `microservice` is accepted as an alias for
`service`.

| Kind | Representative route | Sample response |
| --- | --- | --- |
| `web` | `GET /hello/Alice` | `Hello, Alice!` |
| `api` | `GET /api/greetings/Alice` | JSON `{"message":"Hello, Alice!"}` |
| `service` | `GET /health` | `ok` |
| `monolith` | `GET /catalog/items` | JSON catalog items |

The API response is `application/json` with body
`{"message":"Hello, Alice!"}`. The monolith response is also
`application/json`, with the sample body
`[{"id":1,"name":"Notebook"},{"id":2,"name":"Pen"}]`.

Run a generated project with the standard Rust toolchain:

~~~bash
cd billing-api
cargo check
cargo run
~~~

In another terminal:

~~~bash
curl http://127.0.0.1:8080/api/greetings/Alice
~~~

Ember supports Spring Boot Actuator-style operational endpoints. They are
opt-in and can be selected individually or all at once:

They can be disabled in `application.yaml`:

~~~yaml
actuator:
  endpoints: [health, live, ready, info]
  security:
    enabled: false
  health:
    checks: [database]
~~~

Use `endpoints: "*"` for all endpoints, or
`actuator.endpoints=health,info` in `application.properties`.

Metrics can be exposed with `metrics`, and actuator endpoints can be protected
with `actuator.security.enabled=true` and a bearer token. Applications can
register checks with `register_health_check!`; configured checks return `DOWN`
and HTTP 503 when they fail.

Ember's application security is opt-in. It supports a shared bearer token,
HTTP Basic authentication, or JWT resource-server validation. JWT can use a
local HS256 secret for development, or discover RSA signing keys from an OIDC
issuer:

~~~yaml
security:
  enabled: true
  hide_unauthorized: false
  jwt:
    enabled: true
    issuer_uri: https://issuer.example.com
    audiences: [ember-api]
    required_scopes: [api.read]
~~~

For local development, replace `issuer_uri` with a development-only
`secret`. A direct `jwk_set_uri` can be used when issuer discovery is not
available; production secrets should be supplied through Ember's environment
configuration.

Application logging is available directly from the prelude. Ember re-exports
structured `tracing` macros, so services do not need to call the foundation
module explicitly:

~~~rust
use ember::prelude::*;

#[service]
struct OrderService;

#[logger]
impl OrderService {
    fn create(&self) {
        info!(operation = "create", "creating order");
        warn!("order has no customer");
    }
}
~~~

`#[logger]` can also be placed on a function. It creates a structured span
with the service type and function name; arguments and `self` are excluded by
default so logging does not accidentally require `Debug` or expose values.

The optional `ember-bootui` dependency uses this configuration. It is disabled
and local-only by default:

~~~yaml
bootui:
  enabled: true
  host: 127.0.0.1
  port: 8091
  path: /bootui
  local_only: true
~~~

The dashboard is added explicitly by the application:

~~~rust
let router = ember_bootui::layer(router, &config.bootui);
~~~

When using Ember's standard runner, add `use ember_bootui as _;` to the
application entrypoint so the optional dependency is linked and appears in the
startup diagnostics.

The active profile can be selected from either supported configuration format:

~~~yaml
ember:
  profiles:
    active: dev

scheduler:
  enabled: true
  tasks:
    cleanup:
      enabled: true
      interval_ms: 60000

startup:
  banner: true
  show_config: true
~~~

Ember then loads `application-dev.yaml` or `application-dev.properties` after
the base configuration. If no active profile is configured, `default` is used.

Services can register a task with `register_scheduled_task!("cleanup", 60000,
CleanupService::cleanup)`. The task is disabled unless `scheduler.enabled` is
`true`; each task can override its interval or be disabled independently.

## Project layout

Applications generated by the CLI use compile-time source discovery:

~~~text
my-app/
├── Cargo.toml
├── build.rs
├── src/resources/application.yaml
├── README.md
├── tests/
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
~~~

The monolith starter additionally includes a bounded `src/main/modules/catalog/`
module; the other starters do not create that directory.

'build.rs' calls 'ember_build::discover("src/main")'. Ember generates the module
tree in 'OUT_DIR', so new Rust files under 'src/main/' participate without adding
manual 'mod' declarations to 'main.rs'. Rust visibility and imports remain
normal and explicit inside those files.

The CLI writes fixed, inspectable files: a Cargo manifest, build script,
configuration defaults under `src/resources/`, a `#[ember::main]` entry point,
standard responsibility directories under `src/main/`, and the shape-specific sample. API and monolith starters add an
application-level Serde dependency for their JSON models; web and service
starters do not.

For Spring-style lifecycle extension points, annotate a struct with
'#[post_processor]' and implement 'BeanPostProcessor'. It receives typed
component metadata before and after initialization. The application context
is intentionally typed metadata plus lifecycle orchestration, rather than a
global 'Any' service locator.

## Guarantees, failures, and escape hatches

Starter selection and validation happen in the CLI before project generation.
Unknown kinds are rejected, existing destinations—including dangling
symlinks—are never overwritten, and the destination name is normalized for
Cargo package naming. Generated source then receives ordinary Rust compiler
and macro diagnostics. At runtime the current runner loads and validates the
supported configuration sources, initializes foundation logging, registers
the generated routes, and uses Ember's current tracing, body-limit, and
graceful-shutdown behavior. A generated startup failure emits the bounded
`application_startup_failed` event with an `error_kind` of `configuration`,
`address`, `lifecycle`, or `web`; it does not print arbitrary provider or
configuration error text. Direct callers of `ember::run` still receive the
typed `StartupError` for application-owned handling.

Generation is a bounded sequence of directory and file writes, not an atomic
transaction. A filesystem error can therefore leave a partially written new
destination; the CLI reports the path and does not remove user files or retry
silently. The repository's focused CLI and non-listener integration checks cover this
preflight and compilation behavior. The live route/shutdown smoke remains a
release-gate check in an environment that permits loopback listeners.

The convention is optional. Edit or replace the generated controllers,
services, providers, and configuration; use `build_router()` and the
`ember::web::axum`/Tower re-exports for lower-level HTTP composition; or
replace `#[ember::main]` with an application-owned Axum/Tokio entry point when
you need a custom server boundary. That entry point must include
`include!(concat!(env!("OUT_DIR"), "/ember_modules.rs"));` or declare equivalent
modules manually so static route registrations remain linked. Tokio is not a
stable public Ember re-export, so an application-owned Tokio entry point should
declare `tokio` directly. Axum can use the `ember::web::axum` re-export or a
direct `axum` dependency. A custom server-builder API is not part of the
current starter contract. Replacing `#[ember::main]` transfers configuration,
startup, and graceful-shutdown ownership to the application; `build_router()`
only builds the router and does not perform that orchestration.

Starter generation is **Current**. The opt-in generated typed-graph MVP is
also **Current**, but it is not enabled by the starter templates and does not
inject values into controllers or `ember::run`. Graph scopes, qualifiers,
automatic controller/lifecycle integration,
typed configuration derives, security stack, database adapters, and
shape-specific middleware presets remain **Next** or **Later** roadmap work.
Runtime filesystem scanning, reflection-based discovery, a global mutable
container, and string-key dependency lookup are **Rejected for now** as the
primary architecture.

## Documentation

Start with the [documentation index](docs/README.md). It contains the
Spring-inspired gap analysis, architecture principles, implementation roadmap,
and usage guides for dependency injection, web handlers, configuration,
testing, errors, and extension points.

## Workspace commands

~~~bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
~~~

The CLI is available from the workspace while Ember is unpublished:

~~~bash
cargo run -p ember-cli -- new my-app
cargo run -p ember-cli -- check
cargo run -p ember-cli -- dev
~~~

`ember dev` is the development watcher. Run it once from the application
directory; it watches `src/`, `tests/`, `examples/`, `Cargo.toml`, and
`Cargo.lock`, then restarts the application when code or configuration changes.
Cargo's incremental compilation means dependencies are reused and rebuilt only
when the manifest or lockfile requires it.

When run from this checkout, 'ember new' writes local path dependencies so the
generated project compiles immediately. Its manifest also declares an empty
`[workspace]` table, so creating the project inside this checkout does not make
it an unlisted member of Ember's workspace. A packaged CLI uses the published
`0.1` dependency fallback.

## Configuration

The standard runner loads supported files from the current directory. The
precedence is defaults, base YAML/YML/properties files, profile
YAML/YML/properties files, sorted `EMBER_*` environment variables, then
explicit loader overrides:

~~~yaml
server:
  host: 127.0.0.1
  port: 8080
logging:
  level: info
  backtrace: off
~~~

The equivalent focused properties form is:

~~~properties
server.host=127.0.0.1
server.port=8080
logging.level=info
logging.backtrace=off
~~~

Set `EMBER_PROFILE=test` to activate the matching YAML, YML, and properties
profile files. For example, `EMBER_SERVER_PORT=9000 cargo run -p hello-world`
changes the listening port without changing source code. `RUST_LOG` remains an
explicit advanced tracing-filter override; invalid directives fall back to the
typed `logging.level` without echoing the directive.

## Crates

* 'ember' is the facade used by application developers.
* 'ember-core' contains framework-neutral lifecycle and metadata types.
* 'ember-foundation' contains the framework-neutral typed logging and backtrace
  policy used by runtime consumers; it depends on no Ember crate.
* 'ember-macros' contains the procedural macros and compile-failure tests.
* 'ember-web' owns Axum, Tokio, Tower, route registration, and graceful
  shutdown while consuming foundation logging.
* 'ember-config' provides typed YAML/YML/properties loading with profile,
  environment, and explicit override precedence.
* 'ember-build' generates the convention-based Rust module tree from
  'build.rs'.
* 'ember-cli' provides 'new', 'dev', and 'check'.

See [docs/architecture.md](docs/architecture.md) for the dependency graph,
trade-offs, risks, roadmap, and the multi-agent review contract. Generated
behavior is described in [docs/generated-code.md](docs/generated-code.md).

## Security defaults

The default listener binds to '127.0.0.1:8080', not all interfaces. Ember
installs a 1 MiB default request body limit and request tracing without logging
request bodies. Authentication, secrets management, database adapters, and
exporters are deliberately outside this MVP.

## License

MIT OR Apache-2.0.
