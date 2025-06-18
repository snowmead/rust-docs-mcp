// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![allow(dead_code)]

//! Graph building functionality for module dependencies.

pub mod builder;
pub mod ext;

// Re-export commonly used types
pub use petgraph::graph::Graph;

use std::collections::HashSet;
use petgraph::{Direction, graph::NodeIndex, visit::{Bfs, EdgeRef}};
use ra_ap_ide::{self as ide};

/// Walker for traversing the graph
pub struct GraphWalker {
    direction: Direction,
    pub nodes_visited: HashSet<NodeIndex>,
}

impl GraphWalker {
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            nodes_visited: HashSet::new(),
        }
    }
    
    pub fn walk_graph<F>(
        &mut self,
        graph: &Graph<Node, Edge>,
        start: NodeIndex,
        mut visitor: F,
    ) -> ()
    where
        F: FnMut(&Edge, &Node, usize) -> bool,
    {
        let mut bfs = Bfs::new(graph, start);
        let mut depth = 0;
        
        while let Some(node_idx) = bfs.next(graph) {
            self.nodes_visited.insert(node_idx);
            
            // Visit edges from this node
            for edge_ref in graph.edges_directed(node_idx, self.direction) {
                let edge = edge_ref.weight();
                let target_idx = edge_ref.target();
                let target_node = &graph[target_idx];
                
                if !visitor(edge, target_node, depth) {
                    return;
                }
            }
            
            depth += 1;
        }
    }
}

use crate::item::Item;

#[derive(Debug, Clone)]
pub struct Node {
    pub item: Item,
}

impl Node {
    pub fn display_path(&self, db: &ide::RootDatabase, edition: ide::Edition) -> String {
        self.item.display_path(db, edition)
    }
    
    pub fn display_name(&self, db: &ide::RootDatabase, edition: ide::Edition) -> String {
        self.item.display_name(db, edition)
    }
    
    pub fn kind_display_name(&self, db: &ide::RootDatabase, edition: ide::Edition) -> crate::item::ItemKindDisplayName {
        self.item.kind_display_name(db, edition)
    }
    
    pub fn visibility(&self, db: &ide::RootDatabase, edition: ide::Edition) -> crate::item::ItemVisibility {
        self.item.visibility(db, edition)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relationship {
    Uses,
    Owns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    pub relationship: Relationship,
}

impl Edge {
    pub fn new(relationship: Relationship) -> Self {
        Self { relationship }
    }
    
    pub fn display_name(&self) -> &'static str {
        match self.relationship {
            Relationship::Uses => "uses",
            Relationship::Owns => "owns",
        }
    }
}

// Constructor constants for convenience
impl Edge {
    pub const OWNS: Edge = Edge { relationship: Relationship::Owns };
    pub const USES: Edge = Edge { relationship: Relationship::Uses };
}