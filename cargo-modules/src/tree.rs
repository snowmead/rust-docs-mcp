// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod builder;

pub use self::builder::TreeBuilder;
use crate::item::Item;
use ra_ap_hir::{self as hir};
use ra_ap_ide::{self as ide};

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
    pub fn build(
        db: &ide::RootDatabase,
        crate_id: &hir::Crate,
        _edition: ide::Edition,
    ) -> anyhow::Result<Self> {
        let builder = TreeBuilder::new(db, *crate_id);
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_creation() {
        let root = 1;
        let children = vec![
            Tree::new(2, vec![]),
            Tree::new(3, vec![Tree::new(4, vec![])]),
        ];
        let tree = Tree::new(root, children);
        
        assert_eq!(tree.node, 1);
        assert_eq!(tree.subtrees.len(), 2);
        assert_eq!(tree.subtrees[0].node, 2);
        assert_eq!(tree.subtrees[1].node, 3);
        assert_eq!(tree.subtrees[1].subtrees[0].node, 4);
    }

    #[test]
    fn test_push_subtree() {
        let mut tree = Tree::new(1, vec![]);
        assert_eq!(tree.subtrees.len(), 0);
        
        tree.push_subtree(Tree::new(2, vec![]));
        assert_eq!(tree.subtrees.len(), 1);
        assert_eq!(tree.subtrees[0].node, 2);
        
        tree.push_subtree(Tree::new(3, vec![]));
        assert_eq!(tree.subtrees.len(), 2);
        assert_eq!(tree.subtrees[1].node, 3);
    }

    #[test]
    fn test_tree_equality() {
        let tree1 = Tree::new(1, vec![Tree::new(2, vec![])]);
        let tree2 = Tree::new(1, vec![Tree::new(2, vec![])]);
        let tree3 = Tree::new(1, vec![Tree::new(3, vec![])]);
        
        assert_eq!(tree1, tree2);
        assert_ne!(tree1, tree3);
    }

    #[test]
    fn test_nested_tree_structure() {
        let leaf = Tree::new(4, vec![]);
        let branch = Tree::new(3, vec![leaf]);
        let subtree = Tree::new(2, vec![branch]);
        let root = Tree::new(1, vec![subtree]);
        
        assert_eq!(root.node, 1);
        assert_eq!(root.subtrees[0].node, 2);
        assert_eq!(root.subtrees[0].subtrees[0].node, 3);
        assert_eq!(root.subtrees[0].subtrees[0].subtrees[0].node, 4);
    }
}
