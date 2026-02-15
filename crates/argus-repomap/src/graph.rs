use std::collections::HashMap;
use std::path::PathBuf;

use petgraph::graph::{DiGraph, NodeIndex};

use crate::parser::{Reference, Symbol};

/// A node in the symbol graph: a symbol annotated with its PageRank score.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::parser::{Symbol, SymbolKind};
/// use argus_repomap::graph::SymbolNode;
///
/// let node = SymbolNode {
///     symbol: Symbol {
///         name: "main".into(),
///         kind: SymbolKind::Function,
///         file: PathBuf::from("src/main.rs"),
///         line: 1,
///         signature: "fn main()".into(),
///         token_cost: 2,
///     },
///     rank: 0.0,
/// };
/// assert_eq!(node.rank, 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct SymbolNode {
    /// The original parsed symbol.
    pub symbol: Symbol,
    /// PageRank score (higher = more important).
    pub rank: f64,
}

/// Directed graph of symbols linked by cross-references, with PageRank ranking.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::parser::{Symbol, SymbolKind, Reference};
/// use argus_repomap::graph::SymbolGraph;
///
/// let symbols = vec![
///     Symbol {
///         name: "caller".into(),
///         kind: SymbolKind::Function,
///         file: PathBuf::from("a.rs"),
///         line: 1,
///         signature: "fn caller()".into(),
///         token_cost: 3,
///     },
///     Symbol {
///         name: "callee".into(),
///         kind: SymbolKind::Function,
///         file: PathBuf::from("b.rs"),
///         line: 1,
///         signature: "fn callee()".into(),
///         token_cost: 3,
///     },
/// ];
/// let refs = vec![
///     Reference {
///         from_file: PathBuf::from("a.rs"),
///         from_symbol: Some("caller".into()),
///         to_name: "callee".into(),
///         line: 2,
///     },
/// ];
/// let mut graph = SymbolGraph::build(symbols, refs);
/// graph.compute_pagerank();
/// let ranked = graph.ranked_symbols();
/// assert!(!ranked.is_empty());
/// ```
pub struct SymbolGraph {
    graph: DiGraph<SymbolNode, ()>,
    #[allow(dead_code)]
    name_to_index: HashMap<String, NodeIndex>,
}

impl SymbolGraph {
    /// Build a graph from extracted symbols and references.
    ///
    /// Each symbol becomes a node. Each reference that resolves to a known
    /// symbol name creates a directed edge from the referencing context to
    /// the referenced symbol.
    pub fn build(symbols: Vec<Symbol>, references: Vec<Reference>) -> Self {
        let mut graph = DiGraph::new();
        let mut name_to_index: HashMap<String, NodeIndex> = HashMap::new();

        for symbol in symbols {
            let name = symbol.name.clone();
            let idx = graph.add_node(SymbolNode { symbol, rank: 0.0 });
            // First symbol with a given name wins
            name_to_index.entry(name).or_insert(idx);
        }

        for reference in &references {
            let Some(&to_idx) = name_to_index.get(&reference.to_name) else {
                continue; // Unresolved reference
            };

            // Find the "from" node: prefer the enclosing symbol, fall back to file-level
            let from_idx = reference
                .from_symbol
                .as_ref()
                .and_then(|name| name_to_index.get(name).copied());

            let Some(from_idx) = from_idx else {
                continue;
            };

            // Don't add self-loops
            if from_idx == to_idx {
                continue;
            }

            graph.add_edge(from_idx, to_idx, ());
        }

        Self {
            graph,
            name_to_index,
        }
    }

    /// Run PageRank (damping=0.85, 20 iterations) and store scores on nodes.
    pub fn compute_pagerank(&mut self) {
        let n = self.graph.node_count();
        if n == 0 {
            return;
        }

        let d: f64 = 0.85;
        let n_f64 = n as f64;
        let base = (1.0 - d) / n_f64;

        // Initialize all ranks to 1/N
        let mut ranks = vec![1.0 / n_f64; n];

        for _ in 0..20 {
            let mut new_ranks = vec![base; n];

            for node_idx in self.graph.node_indices() {
                let i = node_idx.index();
                let out_degree = self
                    .graph
                    .neighbors_directed(node_idx, petgraph::Direction::Outgoing)
                    .count();

                if out_degree == 0 {
                    continue;
                }

                let contribution = d * ranks[i] / out_degree as f64;
                for neighbor in self
                    .graph
                    .neighbors_directed(node_idx, petgraph::Direction::Outgoing)
                {
                    new_ranks[neighbor.index()] += contribution;
                }
            }

            ranks = new_ranks;
        }

        // Write ranks back to nodes
        for node_idx in self.graph.node_indices() {
            self.graph[node_idx].rank = ranks[node_idx.index()];
        }
    }

    /// Get all symbols sorted by rank (highest first).
    pub fn ranked_symbols(&self) -> Vec<&SymbolNode> {
        let mut nodes: Vec<&SymbolNode> = self.graph.node_weights().collect();
        nodes.sort_by(|a, b| {
            b.rank
                .partial_cmp(&a.rank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        nodes
    }

    /// Get symbols ranked with a 2x boost for those in the given focus files.
    pub fn ranked_symbols_for_files(&self, focus_files: &[PathBuf]) -> Vec<&SymbolNode> {
        let mut scored: Vec<(&SymbolNode, f64)> = self
            .graph
            .node_weights()
            .map(|node| {
                let multiplier = if focus_files.contains(&node.symbol.file) {
                    2.0
                } else {
                    1.0
                };
                (node, node.rank * multiplier)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(node, _)| node).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::SymbolKind;

    fn make_symbol(name: &str, file: &str) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            file: PathBuf::from(file),
            line: 1,
            signature: format!("fn {name}()"),
            token_cost: 5,
        }
    }

    fn make_ref(from: &str, to: &str) -> Reference {
        Reference {
            from_file: PathBuf::from("test.rs"),
            from_symbol: Some(from.to_string()),
            to_name: to.to_string(),
            line: 1,
        }
    }

    #[test]
    fn pagerank_linked_chain() {
        // A -> B -> C: C should have highest rank (most "votes" flow to it)
        let symbols = vec![
            make_symbol("A", "a.rs"),
            make_symbol("B", "b.rs"),
            make_symbol("C", "c.rs"),
        ];
        let refs = vec![make_ref("A", "B"), make_ref("B", "C")];

        let mut graph = SymbolGraph::build(symbols, refs);
        graph.compute_pagerank();
        let ranked = graph.ranked_symbols();

        assert_eq!(ranked.len(), 3);
        // C (end of chain, receives most links) should rank highest
        assert_eq!(ranked[0].symbol.name, "C");
        assert!(ranked[0].rank > ranked[2].rank);
    }

    #[test]
    fn disconnected_nodes_get_base_rank() {
        let symbols = vec![make_symbol("X", "x.rs"), make_symbol("Y", "y.rs")];
        let refs: Vec<Reference> = vec![];

        let mut graph = SymbolGraph::build(symbols, refs);
        graph.compute_pagerank();
        let ranked = graph.ranked_symbols();

        // Both should have equal base rank = (1-d)/N
        assert_eq!(ranked.len(), 2);
        let diff = (ranked[0].rank - ranked[1].rank).abs();
        assert!(diff < 1e-10, "disconnected nodes should have equal rank");
    }

    #[test]
    fn focus_files_boost_ranking() {
        let symbols = vec![make_symbol("A", "a.rs"), make_symbol("B", "b.rs")];
        let refs: Vec<Reference> = vec![];

        let mut graph = SymbolGraph::build(symbols, refs);
        graph.compute_pagerank();

        // Without focus, both should be equal
        let ranked = graph.ranked_symbols();
        assert!((ranked[0].rank - ranked[1].rank).abs() < 1e-10);

        // With focus on b.rs, B should rank higher
        let focus = vec![PathBuf::from("b.rs")];
        let boosted = graph.ranked_symbols_for_files(&focus);
        assert_eq!(boosted[0].symbol.name, "B");
    }

    #[test]
    fn empty_graph() {
        let mut graph = SymbolGraph::build(vec![], vec![]);
        graph.compute_pagerank();
        let ranked = graph.ranked_symbols();
        assert!(ranked.is_empty());
    }
}
