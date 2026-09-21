# Changelog

## 0.1.2, unreleased

- Add `cache list`, `cache add`, and `cache update` commands for blocking cache
  management from the shell. Updates retain old versions and feature selections.
- Store documentation, dependency metadata, and search indexes per feature
  selection. Add `features`, `no_default_features`, and `all_features` to query
  tools and the matching controls to caching tools.
- Preserve the automatic all/default/no-default feature fallback when options
  are omitted. Explicit requests do not fall back. Recognize rustdoc
  `compile_error!` failures when deciding whether to try defaults.
- Include public imports in exact and fuzzy search, with alias paths and target
  information in item details. Apply the index tokenizer to fuzzy queries and
  require search terms to match even when crate/member filters are present.
- Use the selected feature set and toolchain for dependency metadata. Resolve
  workspace member package identities and Cargo dependency IDs from metadata.
- Report the tested nightly, configured override, and required JSON format in
  `--version`. Add actionable compiler-version errors. Retain
  `RUST_DOCS_MCP_TOOLCHAIN` and rustdoc JSON compatibility validation.

Old docs have no feature provenance and are regenerated on demand from cached
source. Queries with omitted feature options select the automatic variant,
rather than the last explicitly cached selection. Pass the same feature options
to item detail/source requests because item IDs differ between variants.

This version is prepared for release. Updating this file does not publish a
crates.io release or GitHub release.
