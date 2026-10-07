# Publishing releases

Pull requests and branch pushes run formatting, workspace checks, Clippy with
warnings denied, and tests. Stable releases publish when a matching tag such
as `v0.1.0` is pushed. Snapshots publish only when manually started from GitHub
Actions.

## One-time setup

1. Create a crates.io API token with permission to publish the Scafra packages.
2. In the GitHub repository settings, add it as the Actions secret
   `CRATES_IO_TOKEN`.

The first successful release claims the package names on crates.io. The names
were checked during setup, but crates.io package names can be claimed by
someone else before the first release; the publish job will fail clearly if
that happens.

Both publishing paths run formatting, workspace, Clippy, and test gates, then
check package manifests and file lists before using the same crates.io token
to publish in dependency order. Each publish verifies the crate after its
dependencies are available. After each upload the workflow waits for crates.io's
index to expose that exact version before moving on to dependents. The stable
path verifies its tag against
`[workspace.package].version`; the snapshot path applies its selected
prerelease version only inside the runner, leaving the branch unchanged.

## Snapshot releases

In GitHub, open **Actions → Publish to crates.io → Run workflow** and enter a
new prerelease version, for example `0.1.0-snapshot.1` or `0.1.0-alpha.1`.
Use a different version for every snapshot because published crate versions
cannot be reused. Users can try a snapshot with an explicit prerelease
requirement:

```toml
[dependencies]
scafra = "=0.1.0-snapshot.1"
```

Snapshots and stable releases share package names, but prerelease versions do
not satisfy ordinary stable version requirements.

## Release steps

1. Set the shared version in the root `Cargo.toml` and commit the release.
2. Push the release commit to the repository's normal release branch.
3. Create and push the matching tag:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

The publish order is defined in `.github/workflows/publish.yml`. Internal
dependencies are published before packages that depend on them. If a run
stops partway through, rerun the failed workflow: package versions already
visible in the crates.io index are skipped, and publication continues with the
first missing package.

## Package names

Scafra publishes its application-facing facade as `scafra` and its companion
packages under the `scafra-*` prefix. Applications can depend on the facade:

```toml
[dependencies]
scafra = "0.1"
```

Then import the framework with `use scafra::prelude::*`. The CLI package is
`scafra-cli` and installs the `scafra` executable.
