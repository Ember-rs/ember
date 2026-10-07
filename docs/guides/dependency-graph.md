# Typed dependency graph

Scafra includes an opt-in build-time dependency graph for applications that
need explicit multi-file provider wiring. The graph is generated from typed
Rust declarations and is intended to make dependency relationships visible
before the application starts.

The graph is currently an MVP and is not enabled by the starter templates. It
does not inject values into controllers or replace `scafra::run` automatically.

## When to use it

Use the graph when an application has explicit providers spread across several
modules and wants validation of the provider relationships during the build.
For a small application, the default `#[service]` and `#[bean]` conventions are
usually enough.

## Important boundaries

- graph discovery happens at build time;
- dependencies are represented by Rust types, not string names;
- the graph does not provide a runtime service locator;
- application-owned startup and router composition remain explicit.

See the implementation and tests in `crates/scafra-build` for the current API
surface and supported graph shapes.
