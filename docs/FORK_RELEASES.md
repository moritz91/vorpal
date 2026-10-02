# Releases from the moritz91 fork

This fork distributes CLI binaries through
[GitHub Releases](https://github.com/moritz91/vorpal/releases). The initial fork
release is `0.10.2+moritz.1`, based on upstream `v0.10.2`. The suffix is SemVer
build metadata: it identifies the fork build without implying an upstream API
version change. It is also a valid Python local-version identifier. Increment
the build number for further fork releases based on the same upstream version.

The npm wrappers retain their upstream package names for local compatibility,
but are marked `private`. This fork does not publish to npm, PyPI, or crates.io.
The inherited Node and Python registry workflows run only in the upstream
repository. Cargo repository metadata and binary download URLs point to this fork.

## Prepare a version

Use Python 3.11 or newer from the fork checkout:

```sh
python scripts/fork_release.py --set-version 0.10.2+moritz.2
python scripts/fork_release.py --tag v0.10.2+moritz.2
cargo metadata --locked --offline --no-deps --format-version 1
```

The script synchronizes the workspace version, internal dependencies, Cargo.lock,
Python manifests, npm manifests and optional platform dependencies. Review and
commit the changes before releasing. External dependency versions are preserved.
Do not rewrite a previously released tag; use a new build number.

## Validate and publish

The **Release fork CLI** workflow can be dispatched on a branch for a build and
test run. Manual dispatch never publishes a release, even when dispatched on a tag.
Pushing a matching fork tag starts publication in `moritz91/vorpal`:

```sh
git push origin codex/path-filter-cpp-calls
git tag v0.10.2+moritz.1
git push origin v0.10.2+moritz.1
```

These commands publish remotely; version preparation alone does not execute them.
The tag must match all checked manifests and use `<upstream-version>+moritz.<build>`.
The release waits for the Linux workspace tests, targeted Windows regression
tests, eight CLI platform builds and the Linux agent bundles. Native optimized
binaries must also pass an indexing smoke test, including the extraction self-check.
Builds use the committed lockfile. The workflow publishes a regular GitHub release with raw
CLI binaries, optional agent signatures and `SHA256SUMS`. No signing secret is
needed for CLI binaries; `VORPAL_AGENT_SIGNING_KEY` enables signed fleet agents.

## Install and verify

Download the appropriate raw binary and `SHA256SUMS` from the fork release.
Asset names match upstream: for example `vorpal-windows-x64.exe` and
`vorpal-linux-x64`. Rename the downloaded CLI to `vorpal.exe` or `vorpal` on
your PATH. `vorpal --version` must display the fork suffix.

On Linux, with the selected binary and checksum file in the same directory:

```sh
grep '  vorpal-linux-x64$' SHA256SUMS | sha256sum --check
chmod +x vorpal-linux-x64
```

On Windows, compare `Get-FileHash .\vorpal-windows-x64.exe -Algorithm SHA256`
with its line in `SHA256SUMS`. After replacing an installed binary, restart
existing MCP processes so they load the new build.
