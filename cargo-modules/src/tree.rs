// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod builder;

pub use self::builder::TreeBuilder;
use ra_ap_hir::{self as hir};
use ra_ap_ide::{self as ide};
use crate::item::Item;

#[derive(Clone, PartialEq, Debug)]
pub struct Tree<N> {
    pub node: N,
    pub subtrees: Vec<Tree<N>>,
}

impl<N> Tree<N> {
    pub fn new(node: N, subtrees: Vec<Tree<N>>) -> Self {
        Self { node, subtrees }
    }

    pub fn push_subtree(&mut self, subtree: Tree<N>) {
        self.subtrees.push(subtree);
    }
}

pub type ModuleTree = Tree<Item>;

impl ModuleTree {
    /// Builds a module tree from a crate
    pub fn build(db: &ide::RootDatabase, crate_id: &hir::Crate, _edition: ide::Edition) -> anyhow::Result<Self> {
        let builder = TreeBuilder::new(db, *crate_id);
        builder.build()
    }
}
