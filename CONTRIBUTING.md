# Contributing to jugaad-rs

Thanks for considering a contribution. This document covers how the
project is set up, the one discipline that matters more than any other
here, and the checklist a PR needs to pass.

## Getting started

```bash
git clone https://github.com/Am1n1602/jugaad-rs.git
cd jugaad-rs
cargo build --workspace
```

`rust-toolchain.toml` pins the compiler version, so `rustup` picks up the
right one automatically - no manual toolchain setup needed.

The workspace has three crates:

- [`crates/jugaad-core`](crates/jugaad-core/) - the library. Almost all
  real work (a new NSE endpoint, a bug fix, a schema quirk) happens here.
- [`crates/jugaad-cli`](crates/jugaad-cli/) - the `jugaad` binary, a thin
  wrapper over `jugaad-core`.
- [`crates/jugaad-rpc`](crates/jugaad-rpc/) - a gRPC server exposing
  `jugaad-core` to non-Rust frontends.

Two more directories sit outside the Cargo workspace:

- [`python/`](python/) - `sauda`, the pip package that bundles the
  `jugaad-rpc` binary. It has its own
  [development flow](python/README.md#development).
- [`clients/`](clients/) - small example gRPC clients in Python and
  Node.js.

## The one rule that matters most: verify live before writing code

NSE's endpoints have no official documentation. Every non-obvious
behavior in this codebase - a date format, a null-vs-omitted field, an
error envelope, a valid parameter list - was confirmed by hitting the
real API first, not inferred from Python's `jugaad-data` source or
guessed from a sample response.

Concretely, before adding or changing anything that talks to NSE:

1. **Hit the real endpoint** with `curl` (most NSE endpoints need a
   cookie warm-up first and a browser-like `User-Agent` - see any
   existing `NseXxx::new()` for the pattern) or the browser's network
   tab, and look at the actual response.
2. **Cross-check `jugaad-data`'s Python source** if it covers the same
   endpoint, but treat it as a hint, not ground truth - this project has
   found real bugs in it (a UTC-shift timestamp bug, a series filter it
   doesn't apply) and real dead URLs it still uses.
3. **Check more than one sample.** A single hand-picked response can
   look consistent and still miss real variability - several bugs here
   (a `"-"` placeholder date, a field that's sometimes a JSON float
   instead of an int, a nullable field a smaller sample didn't show)
   were only caught by testing a large real response or running the live
   integration test, not the unit test built from one sample.
4. **Record what you found** in
   [`docs/nse-findings.md`](docs/nse-findings.md) - just the finding
   itself (the endpoint, the quirk, the confirmed values), not the
   implementation reasoning. That file is a reference for "what does
   this endpoint actually do," not a design log.

If you can't verify something live (an endpoint that's only populated
during a specific window, for instance), say so and leave it
unimplemented rather than guessing the shape.

## Adding a new NSE endpoint

The established shape, followed throughout this codebase:

1. Verify it live (above).
2. Model the response as a `pub struct` with clean, `snake_case` Rust
   field names, using `#[serde(rename(deserialize = "..."))]` for NSE's
   own field names. Use `Option<T>` for anything confirmed-nullable,
   and a small `deserialize_with` helper for anything that needs
   converting (a two-digit year, a string-encoded number, a `"-"`
   placeholder that should become `None`).
3. Add a unit test built from a real captured JSON response - trimmed to
   what the struct actually keeps, not the full raw payload.
4. Add a `#[ignore = "hits live NSE"]` integration test in
   [`crates/jugaad-core/tests/nse.rs`](crates/jugaad-core/tests/nse.rs)
   and confirm it actually passes against the real endpoint.
5. Core (`_raw`) first. A `_csv` method and CLI wiring are a natural
   separate follow-up, not something every PR needs to include.

## Before opening a PR

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
```

This is exactly what CI runs (`RUSTFLAGS="-D warnings"`, so any clippy
warning fails the build, not just `cargo test` failures). If you touched
anything network-facing, also run the live integration tests for the
part you changed:

```bash
cargo test -p jugaad-core -- --ignored <test_name>
```

If you touched `python/`, also run its offline smoke test (see
[`python/README.md`](python/README.md#development)). And add a line under
**Unreleased** in [`CHANGELOG.md`](CHANGELOG.md) for anything a user of
the library, CLI, server or Python package would notice.

## Code style

- `unsafe_code = "forbid"` at the workspace level - don't introduce any,
  including indirectly through a dependency that requires it.
- No comments explaining *what* code does - names should already make
  that clear. A comment is only worth adding for a non-obvious *why*: a
  hidden NSE constraint, a workaround for a specific confirmed bug,
  something that would surprise a reader.
- No speculative abstractions - don't add configuration, traits, or
  generality for a use case that doesn't exist yet. Three similar lines
  beat a premature helper.
- Keep new dependencies to a minimum, and check they don't collide with
  an existing pinned major version (this bit a real network-resilience
  PR once - `reqwest-middleware`'s latest wanted `reqwest 0.13` against
  this workspace's pinned `0.12`, resolved by pinning an older,
  compatible `reqwest-middleware` version instead of bumping `reqwest`
  workspace-wide for one new dependency).

## Commit messages

Describe what changed and why, not just what. The `Step N: ...` prefix
on commits in this repo's history is the maintainer's own running
sequence for the primary line of work - contributors don't need to
follow that numbering, just write a clear, descriptive message.

## Releasing

For maintainers. Releases are tag-driven, on two independent tracks with
different tag prefixes: a `v*` tag never publishes to PyPI, and a `py-v*`
tag never builds binaries or the Docker image.

| Tag | Workflow | Publishes |
|---|---|---|
| `vX.Y.Z` | `release.yml` | the `jugaad` CLI archives (Linux x86_64, macOS Intel and Apple Silicon, Windows x64), attached to the GitHub Release |
| `vX.Y.Z` | `docker-publish.yml` | `ghcr.io/am1n1602/jugaad-rpc`, tagged `X.Y.Z`, `latest` and the commit SHA |
| `py-vX.Y.Z` | `python-publish.yml` | `sauda` wheels and a source distribution (sdist) to TestPyPI, then to PyPI after a manual approval |

**Version numbers:**

- **`v*` releases:** bump `[workspace.package] version` in `Cargo.toml`
  first, run `cargo build` to refresh `Cargo.lock`, and update the sample
  `jugaad version` output in `docs/cli.md`. The CLI's `--version` reads
  `Cargo.toml`: `v0.2.1` and `v0.2.2` were tagged without a bump, so
  their binaries report `0.2.0`.
- **`py-v*` releases:** `version` in `python/pyproject.toml` is
  independent of the Rust version. PyPI and TestPyPI never accept the same
  version twice, even after a deletion, so always bump before tagging.
  While below 1.0, `sauda` takes a patch bump for compatible changes
  (`0.1.2`, then `0.1.3`) and a minor bump for breaking ones (`0.2.0`), so
  that pins like `~=0.1.3` don't pull users across a break.
- **Which tag reaches whom:** the `sauda` wheel bundles `jugaad-rpc` as
  built from the tagged commit, and the Docker image is rebuilt only on
  `v*` tags. A server change therefore reaches Python users with the next
  `py-v*` tag and Docker users with the next `v*` tag.
- **Wheels and an sdist, always both:** PyPI's
  [packaging guide](https://packaging.python.org/en/latest/discussions/package-formats/)
  says to upload both, and the sdist is what pip builds from on a platform
  with no wheel. The workflow's `sdist` job installs from the sdist and runs
  the smoke test, so a broken sdist fails the release before anything is
  uploaded. Keep the Cargo workspace `members` written without trailing
  slashes (`crates/jugaad-core`, not `crates/jugaad-core/`): maturin trims
  `members` to the crates in the sdist by string comparison, and drops the
  list entirely if they do not match, which leaves a workspace Cargo
  refuses to build.

**Steps:**

1. Land the changes on `main` with CI green.
2. Move the **Unreleased** entries in `CHANGELOG.md` under a new version
   heading with the date.
3. Bump the version as above, commit, and push to `main`.
4. Tag from `main` and push the tag:

   ```bash
   git tag -a vX.Y.Z -m "short description of the release"
   git push origin vX.Y.Z
   ```

   For the Python package, use `py-vX.Y.Z` instead.
5. Watch the run in the Actions tab. For `py-v*`, approve the `pypi`
   deployment when prompted, after checking the TestPyPI upload.
6. Verify what shipped: the Release page has four archives,
   `docker pull ghcr.io/am1n1602/jugaad-rpc:X.Y.Z` works, and
   `pip install sauda==X.Y.Z` works in a clean virtual environment (the
   PyPI JSON API can lag a few minutes behind a new upload).

**One-time setup:** the PyPI and TestPyPI trusted publishers are already
configured (project `sauda`, owner `Am1n1602`, repo `jugaad-rs`, workflow
`python-publish.yml`, environments `pypi` and `testpypi`); redo them only
if the repo or workflow file is renamed. Also give the `pypi` environment
a required reviewer in the repo's GitHub settings - that is what makes the
approval prompt in step 5 appear.
