use std::sync::Arc;

use rust_docs_mcp::docs::query::DocQuery;
use rust_docs_mcp::search::index_types::IndexCrate;
use rust_docs_mcp::search::{FuzzySearchOptions, FuzzySearcher, SearchIndexer};

fn reexport_fixture() -> serde_json::Value {
    serde_json::json!({
        "root": 0,
        "crate_version": "0.1.0",
        "includes_private": false,
        "index": {
            "0": {
                "id": 0, "crate_id": 0, "name": "shared", "span": null,
                "visibility": "public", "docs": null, "links": {},
                "attrs": [], "deprecation": null,
                "inner": {"module": {"is_crate": true, "items": [1], "is_stripped": false}}
            },
            "1": {
                "id": 1, "crate_id": 0, "name": null, "span": null,
                "visibility": "public", "docs": null, "links": {},
                "attrs": [], "deprecation": null,
                "inner": {"use": {
                    "source": "graphql_client", "name": "graphql_client",
                    "id": 2, "is_glob": false
                }}
            }
        },
        "paths": {
            "0": {"crate_id": 0, "path": ["shared"], "kind": "module"},
            "2": {"crate_id": 1, "path": ["graphql_client"], "kind": "module"}
        },
        "external_crates": {
            "1": {"name": "graphql_client", "html_root_url": null, "path": "libgraphql_client.rlib"}
        },
        "target": {"triple": "x86_64-unknown-linux-gnu", "target_features": []},
        "format_version": rustdoc_types::FORMAT_VERSION
    })
}

#[test]
fn exact_search_finds_reexport_without_top_level_name() {
    let data = serde_json::from_value(reexport_fixture()).unwrap();
    let query = DocQuery::new(Arc::new(data));
    let items = query.search_items("graphql_client");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "1");
    assert_eq!(items[0].kind, "use");
    assert_eq!(items[0].path, ["shared", "graphql_client"]);
    let details = query.get_item_details(1).unwrap();
    assert_eq!(
        details.signature.as_deref(),
        Some("pub use graphql_client;")
    );
    let target = details.reexport.unwrap();
    assert_eq!(target.target_crate.as_deref(), Some("graphql_client"));
    assert_eq!(target.target_path.unwrap(), ["graphql_client"]);
    assert!(query.search_items("GraphQLQuery").is_empty());
}

#[test]
fn both_indexers_find_reexport_without_top_level_name() {
    for trimmed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut indexer = SearchIndexer::new_at_path(dir.path()).unwrap();
        if trimmed {
            let data: IndexCrate = serde_json::from_value(reexport_fixture()).unwrap();
            indexer
                .add_index_crate_items("shared", "0.1.0", &data, None)
                .unwrap();
        } else {
            let data = serde_json::from_value(reexport_fixture()).unwrap();
            indexer
                .add_crate_items("shared", "0.1.0", &data, None)
                .unwrap();
        }
        let searcher = FuzzySearcher::from_indexer(&indexer).unwrap();
        let results = searcher
            .search("graphql_client", &FuzzySearchOptions::default())
            .unwrap();
        assert_eq!(results.len(), 1, "trimmed={trimmed}");
        assert_eq!(results[0].name, "graphql_client");
        assert_eq!(results[0].kind, "use");
        assert_eq!(results[0].path, "shared::graphql_client");
        let filtered = FuzzySearchOptions {
            crate_filter: Some("shared".to_owned()),
            ..Default::default()
        };
        assert!(
            searcher
                .search("GraphQLQuery", &filtered)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn renamed_imports_keep_the_public_alias_and_target() {
    let mut fixture = reexport_fixture();
    fixture["index"]["1"]["inner"]["use"]["name"] = serde_json::json!("gql");
    let query = DocQuery::new(Arc::new(serde_json::from_value(fixture).unwrap()));
    let items = query.search_items("gql");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, ["shared", "gql"]);
    let details = query.get_item_details(1).unwrap();
    assert_eq!(
        details.signature.as_deref(),
        Some("pub use graphql_client as gql;")
    );
    assert_eq!(details.reexport.unwrap().source, "graphql_client");
}
