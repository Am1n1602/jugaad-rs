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
