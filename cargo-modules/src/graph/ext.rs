// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extension traits for graph functionality.

use petgraph::graph::{Graph, NodeIndex};

/// Extension trait for Graph with additional utility methods
pub trait GraphExt<N, E> {
    fn contains_node(&self, node_idx: NodeIndex) -> bool;
}

impl<N, E> GraphExt<N, E> for Graph<N, E> {
    fn contains_node(&self, node_idx: NodeIndex) -> bool {
        self.node_weight(node_idx).is_some()
    }
}
