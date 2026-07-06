# `ra_ap_project_model` patch (0.0.289)

Vendored copy of [rust-analyzer](https://github.com/rust-lang/rust-analyzer)'s
`project-model` crate, wired in via `[patch.crates-io]` in the workspace
[`Cargo.toml`](../../Cargo.toml).

## Why this exists

`cargo-modules` (crate name: `rust-analyzer-modules`) pins the `ra_ap_*` stack at
**0.0.289**. That release calls:

```text
cargo metadata ... --lockfile-path <path>
```

for toolchains ≥ 1.82. **Cargo 1.95+ removed `--lockfile-path`**, so structure
analysis (`analyze_crate_structure`) fails on modern nightlies with:

```text
error: unexpected argument '--lockfile-path' found
```

Upgrading all of `ra_ap_*` to a release that includes the upstream fix (e.g.
0.0.331+) is a larger effort: `cargo-modules` does not compile against those
APIs without substantial rewrites.

This patch keeps **0.0.289** everywhere else and backports only the lockfile-path
handling from upstream.

## What we changed

The only functional diff from crates.io `ra_ap_project_model` 0.0.289 is in
[`src/cargo_workspace.rs`](src/cargo_workspace.rs):

- For **rustc/cargo ≥ 1.95**: use `-Zlockfile-path` and set
  `CARGO_RESOLVER_LOCKFILE_PATH` (Cargo's replacement API).
- For **older toolchains**: keep the original `--lockfile-path` behavior.

Upstream references:

- [rust-analyzer#21715](https://github.com/rust-lang/rust-analyzer/issues/21715)
- [rust-analyzer#21783](https://github.com/rust-lang/rust-analyzer/issues/21783)

## When to remove this patch

Delete `patches/ra_ap_project_model/` and the `[patch.crates-io]` entry when
either:

1. `cargo-modules` is upgraded to an `ra_ap_*` release that includes the
   lockfile-path fix natively, or
2. The pinned nightly/toolchain is downgraded below Cargo 1.95 (unlikely).

After removal, run `cargo test -p rust-docs-mcp --test integration_tests
test_structure` to confirm structure analysis still works.

## Maintenance

- Do **not** re-copy the full crate from the registry without trimming upstream
  test fixtures; this tree is library-only (no `test_data/`, no `tests.rs`).
- If you bump `nightly-*` in `rust-toolchain.toml`, re-check structure analysis
  and this lockfile-path logic.
