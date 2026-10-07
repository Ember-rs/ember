# Publishing releases

This workspace publishes its packages to crates.io when a version tag such as
`v0.1.0` is pushed.

## One-time setup

1. Create a crates.io API token with permission to publish the Scafra packages.
2. In the GitHub repository settings, add it as the Actions secret
   `CRATES_IO_TOKEN`.

The workflow does not run on ordinary branch pushes. It checks that the tag
matches `[workspace.package].version`, checks the workspace, verifies each
package's file list, then publishes packages in dependency order.

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
