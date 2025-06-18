use anyhow::{Context, Result};
use rmcp::schemars;
use rustdoc_types::{Crate, Id, Item, ItemEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Query interface for rustdoc JSON data
#[derive(Debug)]
pub struct DocQuery {
    crate_data: Crate,
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
    pub generics: Option<serde_json::Value>,
    pub fields: Option<Vec<ItemInfo>>,
    pub variants: Option<Vec<ItemInfo>>,
    pub methods: Option<Vec<ItemInfo>>,
    pub source_location: Option<SourceLocation>,
}

impl DocQuery {
    /// Create a new query interface for a crate's documentation
    pub fn new(crate_data: Crate) -> Self {
        Self { crate_data }
    }

    /// List all items in the crate, optionally filtered by kind
    pub fn list_items(&self, kind_filter: Option<&str>) -> Vec<ItemInfo> {
        let mut items = Vec::new();

        for (id, item) in &self.crate_data.index {
            if let Some(filter) = &kind_filter {
                if self.get_item_kind_string(&item.inner) != *filter {
                    continue;
                }
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
            if let Some(name) = &item.name {
                if name.to_lowercase().contains(&pattern_lower) {
                    if let Some(info) = self.item_to_info(id, item) {
                        items.push(info);
                    }
                }
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
        let name = item.name.clone()?;
        let kind = self.get_item_kind_string(&item.inner);
        let path = self.get_item_path(id);
        let visibility = self.get_visibility_string(&item.visibility);

        Some(ItemInfo {
            id: id.0.to_string(),
            name,
            kind,
            path,
            docs: item.docs.clone(),
            visibility,
        })
    }

    /// Get the kind of an item as a string
    fn get_item_kind_string(&self, inner: &ItemEnum) -> String {
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
        .to_string()
    }

    /// Get the full path of an item
    fn get_item_path(&self, id: &Id) -> Vec<String> {
        if let Some(summary) = self.crate_data.paths.get(id) {
            summary.path.clone()
        } else {
            Vec::new()
        }
    }

    /// Get visibility as a string
    fn get_visibility_string(&self, vis: &rustdoc_types::Visibility) -> String {
        use rustdoc_types::Visibility::*;
        match vis {
            Public => "public".to_string(),
            Default => "default".to_string(),
            Crate => "crate".to_string(),
            Restricted { parent, .. } => format!("restricted({})", parent.0),
        }
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
                Some(format!("fn {}{}{}{}", name, generics, params, output))
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
    fn format_fn_params(&self, params: &Vec<(String, rustdoc_types::Type)>) -> String {
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
                            name: format!("(field {} stripped)", i),
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
    fn get_trait_items(&self, items: &Vec<Id>) -> Vec<ItemInfo> {
        items
            .iter()
            .filter_map(|item_id| {
                let item = self.crate_data.index.get(item_id)?;
                self.item_to_info(item_id, item)
            })
            .collect()
    }

    /// Get impl items as ItemInfo
    fn get_impl_items(&self, items: &Vec<Id>) -> Vec<ItemInfo> {
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
