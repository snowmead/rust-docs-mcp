//! Types for indexing rustdoc JSON without allocating function signatures,
//! generics, or implementation bodies. Only `use` and module payloads survive
//! deserialization so import names and their public paths can be indexed.

use rustdoc_types::{Id, ItemSummary, Visibility};
use serde::Deserialize;
use std::collections::HashMap;

/// The root and item/path maps needed to index names and public import paths.
#[derive(Deserialize)]
pub struct IndexCrate {
    pub root: Id,
    pub index: HashMap<Id, IndexItem>,
    pub paths: HashMap<Id, ItemSummary>,
}

/// Item metadata plus the small import/module payloads needed for path discovery.
pub struct IndexItem {
    pub name: Option<String>,
    pub docs: Option<String>,
    pub visibility: Visibility,
    /// The item kind used as the search facet.
    pub kind_tag: String,
    pub import: Option<rustdoc_types::Use>,
    pub module: Option<rustdoc_types::Module>,
}

impl<'de> Deserialize<'de> for IndexItem {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawItem {
            name: Option<String>,
            docs: Option<String>,
            visibility: Visibility,
            #[serde(deserialize_with = "deserialize_inner")]
            inner: IndexInner,
        }
        let raw = RawItem::deserialize(deserializer)?;
        Ok(Self {
            name: raw
                .name
                .or_else(|| raw.inner.import.as_ref().map(|i| i.name.clone())),
            docs: raw.docs,
            visibility: raw.visibility,
            kind_tag: raw.inner.kind,
            import: raw.inner.import,
            module: raw.inner.module,
        })
    }
}

#[derive(Default)]
struct IndexInner {
    kind: String,
    import: Option<rustdoc_types::Use>,
    module: Option<rustdoc_types::Module>,
}

impl IndexCrate {
    pub fn reexport_paths(&self) -> std::collections::HashMap<Id, Vec<String>> {
        crate::docs::query::module_reexport_paths(
            self.root,
            |id| {
                self.index.get(&id).and_then(|item| {
                    item.module
                        .as_ref()
                        .map(|module| (item.name.as_deref().unwrap_or(""), module.items.as_slice()))
                })
            },
            |id| {
                self.index
                    .get(&id)
                    .and_then(|item| item.import.as_ref().map(|import| import.name.as_str()))
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Skip payloads that the indexer does not need.
// ---------------------------------------------------------------------------

/// Map a JSON variant-tag string to the same `&'static str` that
/// [`crate::docs::query::item_kind_str`] returns for the corresponding
/// [`rustdoc_types::ItemEnum`] variant.
///
/// Uses `_ => "unknown"` so that new variants added in future
/// `rustdoc-types` releases don't cause deserialization failures.
fn tag_to_kind(tag: &str) -> String {
    match tag {
        "module" => "module",
        "struct" => "struct",
        "enum" => "enum",
        "function" => "function",
        "trait" => "trait",
        "impl" => "impl",
        "type_alias" => "type_alias",
        "constant" => "constant",
        "static" => "static",
        "macro" => "macro",
        "extern_crate" => "extern_crate",
        "use" => "use",
        "union" => "union",
        // `ItemEnum::StructField` → snake_case is `struct_field`, but
        // `item_kind_str` returns `"field"`.
        "struct_field" => "field",
        "variant" => "variant",
        "trait_alias" => "trait_alias",
        "proc_macro" => "proc_macro",
        "primitive" => "primitive",
        "assoc_const" => "assoc_const",
        "assoc_type" => "assoc_type",
        "extern_type" => "extern_type",
        _ => "unknown",
    }
    .to_string()
}

/// Read the variant tag, preserving only import and module payloads.
///
/// Rustdoc JSON uses serde's default externally-tagged enum encoding:
///
/// - **Newtype/struct variants** (most): `{"function": { ... }}`
///   Keep imports/modules; skip every other value with `IgnoredAny`.
/// - **Unit variants** (`ExternType`): `"extern_type"`
///   → handled by `visit_str`: return the tag directly.
fn deserialize_inner<'de, D>(deserializer: D) -> Result<IndexInner, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct KindTagVisitor;

    impl<'de> serde::de::Visitor<'de> for KindTagVisitor {
        type Value = IndexInner;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an externally-tagged ItemEnum variant (map or string)")
        }

        // Unit variant: `"extern_type"`
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<IndexInner, E> {
            Ok(IndexInner {
                kind: tag_to_kind(v),
                ..Default::default()
            })
        }

        // Newtype/struct variant: `{"function": { ... }}`
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<IndexInner, A::Error> {
            let key: String = map
                .next_key()?
                .ok_or_else(|| serde::de::Error::custom("empty map for ItemEnum inner"))?;
            let mut inner = IndexInner {
                kind: tag_to_kind(&key),
                ..Default::default()
            };
            match key.as_str() {
                "use" => inner.import = Some(map.next_value()?),
                "module" => inner.module = Some(map.next_value()?),
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
            Ok(inner)
        }
    }

    deserializer.deserialize_any(KindTagVisitor)
}

#[cfg(test)]
fn deserialize_kind_tag<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    deserialize_inner(deserializer).map(|inner| inner.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that every known `ItemEnum` variant tag maps to the expected
    /// kind string, matching what `item_kind_str` would return.
    #[test]
    fn tag_to_kind_covers_all_variants() {
        let cases = [
            ("module", "module"),
            ("struct", "struct"),
            ("enum", "enum"),
            ("function", "function"),
            ("trait", "trait"),
            ("impl", "impl"),
            ("type_alias", "type_alias"),
            ("constant", "constant"),
            ("static", "static"),
            ("macro", "macro"),
            ("extern_crate", "extern_crate"),
            ("use", "use"),
            ("union", "union"),
            ("struct_field", "field"),
            ("variant", "variant"),
            ("trait_alias", "trait_alias"),
            ("proc_macro", "proc_macro"),
            ("primitive", "primitive"),
            ("assoc_const", "assoc_const"),
            ("assoc_type", "assoc_type"),
            ("extern_type", "extern_type"),
        ];
        for (tag, expected) in cases {
            assert_eq!(tag_to_kind(tag), expected, "mismatch for tag '{tag}'");
        }
    }

    /// Unknown variant tags should fall back to `"unknown"` for
    /// forward-compatibility with new `rustdoc-types` releases.
    #[test]
    fn tag_to_kind_unknown_falls_back() {
        assert_eq!(tag_to_kind("future_variant"), "unknown");
        assert_eq!(tag_to_kind(""), "unknown");
    }

    /// Deserialise a newtype/struct variant `{"function": {...}}`.
    #[test]
    fn deserialize_kind_tag_map_variant() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_kind_tag")]
            inner: String,
        }
        let json = r#"{"inner": {"function": {"sig": {}, "generics": {}}}}"#;
        let w: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(w.inner, "function");
    }

    /// Deserialise a unit variant `"extern_type"`.
    #[test]
    fn deserialize_kind_tag_string_variant() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_kind_tag")]
            inner: String,
        }
        let json = r#"{"inner": "extern_type"}"#;
        let w: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(w.inner, "extern_type");
    }

    /// Unknown variant tag deserialises as `"unknown"` without error.
    #[test]
    fn deserialize_kind_tag_unknown_variant() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_kind_tag")]
            inner: String,
        }
        let json = r#"{"inner": {"future_variant": {"x": 1}}}"#;
        let w: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(w.inner, "unknown");
    }
}
