use anyhow::{Context, Result};
use rmcp::schemars;
use rustdoc_types::{Crate, Id, Item, ItemEnum, Visibility};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

/// Return the kind of an item as a borrowed `&'static str`.
///
/// This is the allocation-free form used by the search indexer hot loop.
/// [`item_kind_string`] delegates to this and adds one allocation for
/// API backward compatibility.
pub fn item_kind_str(inner: &ItemEnum) -> &'static str {
    use ItemEnum::*;
    match inner {
        Module(_) => "module",
        Struct(_) => "struct",
        Enum(_) => "enum",
        Function(_) => "function",
        Trait(_) => "trait",
        Impl(_) => "impl",
        TypeAlias(_) => "type_alias",
        Constant { .. } => "constant",
        Static(_) => "static",
        Macro(_) => "macro",
        ExternCrate { .. } => "extern_crate",
        Use(_) => "use",
        Union(_) => "union",
        StructField(_) => "field",
        Variant(_) => "variant",
        TraitAlias(_) => "trait_alias",
        ProcMacro(_) => "proc_macro",
        Primitive(_) => "primitive",
        AssocConst { .. } => "assoc_const",
        AssocType { .. } => "assoc_type",
        ExternType => "extern_type",
    }
}

/// Return the kind string for an item's inner enum (e.g. "struct", "function").
///
/// Kept as a convenience for [`DocQuery`] callers that serialize `ItemInfo`.
/// Prefer [`item_kind_str`] in allocation-sensitive code paths.
pub fn item_kind_string(inner: &ItemEnum) -> String {
    item_kind_str(inner).to_string()
}

/// Resolve names stored inside `use` items as well as ordinary item names.
pub fn item_name<'a>(item: &'a Item, path: &'a [String]) -> Option<&'a str> {
    item.name.as_deref().or_else(|| match &item.inner {
        ItemEnum::Use(import) => Some(import.name.as_str()),
        _ => path.last().map(String::as_str),
    })
}

/// Build import paths by walking module membership once. Import IDs are often
/// absent from rustdoc's canonical paths map, especially for external crates.
pub fn reexport_paths(crate_data: &Crate) -> std::collections::HashMap<Id, Vec<String>> {
    module_reexport_paths(
        crate_data.root,
        |id| {
            crate_data
                .index
                .get(&id)
                .and_then(|item| match &item.inner {
                    ItemEnum::Module(module) => {
                        Some((item.name.as_deref().unwrap_or(""), module.items.as_slice()))
                    }
                    _ => None,
                })
        },
        |id| {
            crate_data
                .index
                .get(&id)
                .and_then(|item| match &item.inner {
                    ItemEnum::Use(import) => Some(import.name.as_str()),
                    _ => None,
                })
        },
    )
}

pub(crate) fn module_reexport_paths<'a>(
    root: Id,
    module: impl Fn(Id) -> Option<(&'a str, &'a [Id])>,
    import_name: impl Fn(Id) -> Option<&'a str>,
) -> std::collections::HashMap<Id, Vec<String>> {
    let mut paths = std::collections::HashMap::new();
    let mut visited = std::collections::HashSet::new();
    let mut pending = vec![(root, Vec::new())];
    while let Some((id, mut path)) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some((name, children)) = module(id) {
            path.push(name.to_owned());
            for child in children {
                if let Some(name) = import_name(*child) {
                    let mut import_path = path.clone();
                    import_path.push(name.to_owned());
                    paths.insert(*child, import_path);
                } else if module(*child).is_some() {
                    pending.push((*child, path.clone()));
                }
            }
        }
    }
    paths
}

/// Return an item's path, including public import aliases.
pub fn item_path(crate_data: &Crate, id: &Id) -> Vec<String> {
    if matches!(
        crate_data.index.get(id).map(|item| &item.inner),
        Some(ItemEnum::Use(_))
    ) {
        return reexport_paths(crate_data).remove(id).unwrap_or_default();
    }
    crate_data
        .paths
        .get(id)
        .map(|summary| summary.path.clone())
        .unwrap_or_default()
}

/// Format an item's visibility without allocating in the common cases.
///
/// `Public`, `Default`, and `Crate` return [`Cow::Borrowed`] static
/// strings (zero allocations). Only `Restricted` allocates (one
/// `String` via `format!`). The indexer hot loop uses this directly;
/// [`visibility_string`] delegates to it for API compatibility.
pub fn visibility_str_cow(vis: &Visibility) -> Cow<'static, str> {
    use Visibility::*;
    match vis {
        Public => Cow::Borrowed("public"),
        Default => Cow::Borrowed("default"),
        Crate => Cow::Borrowed("crate"),
        Restricted { parent, .. } => Cow::Owned(format!("restricted({})", parent.0)),
    }
}

/// Format an item's visibility as a human-readable string.
///
/// Prefer [`visibility_str_cow`] in allocation-sensitive code paths.
pub fn visibility_string(vis: &Visibility) -> String {
    visibility_str_cow(vis).into_owned()
}

/// Build an [`ItemInfo`] for a single rustdoc item without owning the full `Crate`.
///
/// This is the indexing-hot-path version of [`DocQuery::item_to_info`]: it
/// takes borrowed state so the search indexer can iterate `crate.index`
/// without first cloning the `Crate` into a `DocQuery`.
pub fn build_item_info(crate_data: &Crate, id: &Id, item: &Item) -> Option<ItemInfo> {
    let path = item_path(crate_data, id);
    let name = item_name(item, &path)?.to_owned();

    Some(ItemInfo {
        id: id.0.to_string(),
        name,
        kind: item_kind_string(&item.inner),
        path,
        docs: item.docs.clone(),
        visibility: visibility_string(&item.visibility),
    })
}

/// Query interface for rustdoc JSON data
#[derive(Debug)]
pub struct DocQuery {
    crate_data: Arc<Crate>,
    reexport_paths: std::collections::HashMap<Id, Vec<String>>,
}

/// Simplified item information for API responses
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ItemInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Vec<String>,
    pub docs: Option<String>,
    pub visibility: String,
}

/// Source location information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceLocation {
    pub filename: String,
    pub line_start: usize,
    pub column_start: usize,
    pub line_end: usize,
    pub column_end: usize,
}

/// Source code information for an item
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceInfo {
    pub location: SourceLocation,
    pub code: String,
    pub context_lines: Option<usize>,
}

/// Detailed item information including signatures
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DetailedItem {
    pub info: ItemInfo,
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reexport: Option<crate::docs::outputs::ReexportInfo>,
    pub generics: Option<serde_json::Value>,
    pub fields: Option<Vec<ItemInfo>>,
    pub variants: Option<Vec<ItemInfo>>,
    pub methods: Option<Vec<ItemInfo>>,
    pub source_location: Option<SourceLocation>,
}

impl DocQuery {
    /// Create a new query interface for a crate's documentation
    pub fn new(crate_data: Arc<Crate>) -> Self {
        let reexport_paths = reexport_paths(&crate_data);
        Self {
            crate_data,
            reexport_paths,
        }
    }

    /// List all items in the crate, optionally filtered by kind
    pub fn list_items(&self, kind_filter: Option<&str>) -> Vec<ItemInfo> {
        let mut items = Vec::new();

        for (id, item) in &self.crate_data.index {
            if let Some(filter) = &kind_filter
                && self.get_item_kind_string(&item.inner) != *filter
            {
                continue;
            }

            if let Some(info) = self.item_to_info(id, item) {
                items.push(info);
            }
        }

        // Sort by path and name for consistent output
        items.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.name.cmp(&b.name)));
        items
    }

    /// Search for items by name pattern
    pub fn search_items(&self, pattern: &str) -> Vec<ItemInfo> {
        let pattern_lower = pattern.to_lowercase();
        let mut items = Vec::new();

        for (id, item) in &self.crate_data.index {
            if let Some(info) = self.item_to_info(id, item)
                && info.name.to_lowercase().contains(&pattern_lower)
            {
                items.push(info);
            }
        }

        items.sort_by(|a, b| {
            // Sort by relevance (exact match first, then prefix match, then contains)
            let a_exact = a.name.to_lowercase() == pattern_lower;
            let b_exact = b.name.to_lowercase() == pattern_lower;
            let a_prefix = a.name.to_lowercase().starts_with(&pattern_lower);
            let b_prefix = b.name.to_lowercase().starts_with(&pattern_lower);

            b_exact
                .cmp(&a_exact)
                .then_with(|| b_prefix.cmp(&a_prefix))
                .then_with(|| a.name.len().cmp(&b.name.len()))
                .then_with(|| a.name.cmp(&b.name))
        });

        items
    }

    /// Get detailed information about a specific item by ID
    pub fn get_item_details(&self, item_id: u32) -> Result<DetailedItem> {
        let id = Id(item_id);
        let item = self.crate_data.index.get(&id).context("Item not found")?;

        let info = self
            .item_to_info(&id, item)
            .context("Failed to convert item to info")?;

        let mut details = DetailedItem {
            info,
            signature: self.get_item_signature(item),
            reexport: match &item.inner {
                ItemEnum::Use(import) => {
                    let summary = import.id.and_then(|id| self.crate_data.paths.get(&id));
                    Some(crate::docs::outputs::ReexportInfo {
                        source: import.source.clone(),
                        target_id: import.id.map(|id| id.0.to_string()),
                        target_path: summary.map(|s| s.path.clone()),
                        target_crate: summary
                            .and_then(|s| self.crate_data.external_crates.get(&s.crate_id))
                            .map(|c| c.name.clone()),
                        is_glob: import.is_glob,
                    })
                }
                _ => None,
            },
            generics: None,
            fields: None,
            variants: None,
            methods: None,
            source_location: self.get_item_source_location(item),
        };

        // Add type-specific information
        match &item.inner {
            ItemEnum::Struct(s) => {
                details.generics = serde_json::to_value(&s.generics).ok();
                details.fields = Some(self.get_struct_fields(s));
            }
            ItemEnum::Enum(e) => {
                details.generics = serde_json::to_value(&e.generics).ok();
                details.variants = Some(self.get_enum_variants(e));
            }
            ItemEnum::Trait(t) => {
                details.generics = serde_json::to_value(&t.generics).ok();
                details.methods = Some(self.get_trait_items(&t.items));
            }
            ItemEnum::Impl(i) => {
                details.generics = serde_json::to_value(&i.generics).ok();
                details.methods = Some(self.get_impl_items(&i.items));
            }
            ItemEnum::Function(f) => {
                details.generics = serde_json::to_value(&f.generics).ok();
            }
            _ => {}
        }

        Ok(details)
    }

    /// Get documentation for a specific item
    pub fn get_item_docs(&self, item_id: u32) -> Result<Option<String>> {
        let id = Id(item_id);
        let item = self.crate_data.index.get(&id).context("Item not found")?;

        Ok(item.docs.clone())
    }

    /// Helper to convert an Item to ItemInfo
    fn item_to_info(&self, id: &Id, item: &Item) -> Option<ItemInfo> {
        let path = self
            .reexport_paths
            .get(id)
            .or_else(|| self.crate_data.paths.get(id).map(|summary| &summary.path))
            .cloned()
            .unwrap_or_default();
        Some(ItemInfo {
            id: id.0.to_string(),
            name: item_name(item, &path)?.to_owned(),
            path,
            kind: item_kind_string(&item.inner),
            docs: item.docs.clone(),
            visibility: visibility_string(&item.visibility),
        })
    }

    /// Get the kind of an item as a string
    fn get_item_kind_string(&self, inner: &ItemEnum) -> String {
        item_kind_string(inner)
    }

    /// Get a signature representation for an item
    fn get_item_signature(&self, item: &Item) -> Option<String> {
        use ItemEnum::*;
        match &item.inner {
            Function(f) => {
                let name = item.name.as_ref()?;
                let generics = self.format_generics(&f.generics);
                let params = self.format_fn_params(&f.sig.inputs);
                let output = self.format_fn_output(&f.sig.output);
                Some(format!("fn {name}{generics}{params}{output}"))
            }
            Use(import) => {
                let suffix = if import.is_glob {
                    "::*".to_owned()
                } else if import.source.rsplit("::").next() != Some(import.name.as_str()) {
                    format!(" as {}", import.name)
                } else {
                    String::new()
                };
                Some(format!(
                    "{}use {}{};",
                    if item.visibility == Visibility::Public {
                        "pub "
                    } else {
                        ""
                    },
                    import.source,
                    suffix
                ))
            }
            _ => None,
        }
    }

    /// Format generic parameters
    fn format_generics(&self, generics: &rustdoc_types::Generics) -> String {
        // Simplified generic formatting
        if generics.params.is_empty() {
            String::new()
        } else {
            "<...>".to_string()
        }
    }

    /// Format function parameters
    fn format_fn_params(&self, params: &[(String, rustdoc_types::Type)]) -> String {
        let param_strs: Vec<String> = params.iter().map(|(name, _)| name.clone()).collect();
        format!("({})", param_strs.join(", "))
    }

    /// Format function output
    fn format_fn_output(&self, output: &Option<rustdoc_types::Type>) -> String {
        output
            .as_ref()
            .map(|_| " -> ...".to_string())
            .unwrap_or_default()
    }

    /// Get struct fields as ItemInfo
    fn get_struct_fields(&self, s: &rustdoc_types::Struct) -> Vec<ItemInfo> {
        use rustdoc_types::StructKind;
        match &s.kind {
            StructKind::Unit => vec![],
            StructKind::Tuple(fields) => fields
                .iter()
                .enumerate()
                .filter_map(|(i, field_id)| {
                    if let Some(field_id) = field_id {
                        let item = self.crate_data.index.get(field_id)?;
                        let mut info = self.item_to_info(field_id, item)?;
                        if info.name.is_empty() {
                            info.name = i.to_string();
                        }
                        Some(info)
                    } else {
                        Some(ItemInfo {
                            id: String::new(),
                            name: format!("(field {i} stripped)"),
                            kind: "field".to_string(),
                            path: Vec::new(),
                            docs: None,
                            visibility: "private".to_string(),
                        })
                    }
                })
                .collect(),
            StructKind::Plain {
                fields,
                has_stripped_fields,
            } => {
                let mut field_infos: Vec<ItemInfo> = fields
                    .iter()
                    .filter_map(|field_id| {
                        let item = self.crate_data.index.get(field_id)?;
                        self.item_to_info(field_id, item)
                    })
                    .collect();

                if *has_stripped_fields {
                    field_infos.push(ItemInfo {
                        id: String::new(),
                        name: "(some fields stripped)".to_string(),
                        kind: "note".to_string(),
                        path: Vec::new(),
                        docs: None,
                        visibility: "private".to_string(),
                    });
                }

                field_infos
            }
        }
    }

    /// Get enum variants as ItemInfo
    fn get_enum_variants(&self, e: &rustdoc_types::Enum) -> Vec<ItemInfo> {
        let mut variant_infos: Vec<ItemInfo> = e
            .variants
            .iter()
            .filter_map(|variant_id| {
                let item = self.crate_data.index.get(variant_id)?;
                self.item_to_info(variant_id, item)
            })
            .collect();

        if e.has_stripped_variants {
            variant_infos.push(ItemInfo {
                id: String::new(),
                name: "(some variants stripped)".to_string(),
                kind: "note".to_string(),
                path: Vec::new(),
                docs: None,
                visibility: "private".to_string(),
            });
        }

        variant_infos
    }

    /// Get trait items as ItemInfo
    fn get_trait_items(&self, items: &[Id]) -> Vec<ItemInfo> {
        items
            .iter()
            .filter_map(|item_id| {
                let item = self.crate_data.index.get(item_id)?;
                self.item_to_info(item_id, item)
            })
            .collect()
    }

    /// Get impl items as ItemInfo
    fn get_impl_items(&self, items: &[Id]) -> Vec<ItemInfo> {
        items
            .iter()
            .filter_map(|item_id| {
                let item = self.crate_data.index.get(item_id)?;
                self.item_to_info(item_id, item)
            })
            .collect()
    }

    /// Get source location information for an item
    fn get_item_source_location(&self, item: &Item) -> Option<SourceLocation> {
        let span = item.span.as_ref()?;
        Some(SourceLocation {
            filename: span.filename.to_string_lossy().to_string(),
            line_start: span.begin.0,
            column_start: span.begin.1,
            line_end: span.end.0,
            column_end: span.end.1,
        })
    }

    /// Get source code for a specific item by ID
    pub fn get_item_source(
        &self,
        item_id: u32,
        base_path: &std::path::Path,
        context_lines: usize,
    ) -> Result<SourceInfo> {
        let id = Id(item_id);
        let item = self.crate_data.index.get(&id).context("Item not found")?;

        let span = item.span.as_ref().context("Item has no source span")?;
        let source_path = base_path.join(&span.filename);

        if !source_path.exists() {
            anyhow::bail!("Source file not found: {}", source_path.display());
        }

        let content = std::fs::read_to_string(&source_path)
            .with_context(|| format!("Failed to read source file: {}", source_path.display()))?;

        let lines: Vec<&str> = content.lines().collect();

        // Calculate line range with context
        let start_line = span.begin.0.saturating_sub(1).saturating_sub(context_lines);
        let end_line = std::cmp::min(span.end.0 + context_lines, lines.len());

        // Extract the relevant lines
        let code_lines: Vec<String> = lines[start_line..end_line]
            .iter()
            .map(|line| line.to_string())
            .collect();

        Ok(SourceInfo {
            location: SourceLocation {
                filename: span.filename.to_string_lossy().to_string(),
                line_start: span.begin.0,
                column_start: span.begin.1,
                line_end: span.end.0,
                column_end: span.end.1,
            },
            code: code_lines.join("\n"),
            context_lines: Some(context_lines),
        })
    }
}
