use crate::graph::SymbolNode;

/// Select the top-ranked symbols that fit within a token budget.
///
/// Symbols must be pre-sorted by rank (highest first). Greedily includes
/// symbols until the next one would exceed `max_tokens`.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::parser::{Symbol, SymbolKind};
/// use argus_repomap::graph::SymbolNode;
/// use argus_repomap::budget::fit_to_budget;
///
/// let nodes = vec![
///     SymbolNode {
///         symbol: Symbol {
///             name: "a".into(),
///             kind: SymbolKind::Function,
///             file: PathBuf::from("a.rs"),
///             line: 1,
///             signature: "fn a()".into(),
///             token_cost: 10,
///         },
///         rank: 1.0,
///     },
///     SymbolNode {
///         symbol: Symbol {
///             name: "b".into(),
///             kind: SymbolKind::Function,
///             file: PathBuf::from("b.rs"),
///             line: 1,
///             signature: "fn b()".into(),
///             token_cost: 10,
///         },
///         rank: 0.5,
///     },
/// ];
/// let refs: Vec<&SymbolNode> = nodes.iter().collect();
/// let selected = fit_to_budget(&refs, 15);
/// assert_eq!(selected.len(), 1);
/// ```
pub fn fit_to_budget<'a>(symbols: &[&'a SymbolNode], max_tokens: usize) -> Vec<&'a SymbolNode> {
    let mut selected = Vec::new();
    let mut used = 0;

    for symbol in symbols {
        let cost = symbol.symbol.token_cost.max(1);
        if used + cost > max_tokens {
            break;
        }
        used += cost;
        selected.push(*symbol);
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Symbol, SymbolKind};
    use std::path::PathBuf;

    fn make_node(name: &str, cost: usize, rank: f64) -> SymbolNode {
        SymbolNode {
            symbol: Symbol {
                name: name.to_string(),
                kind: SymbolKind::Function,
                file: PathBuf::from("test.rs"),
                line: 1,
                signature: format!("fn {name}()"),
                token_cost: cost,
            },
            rank,
        }
    }

    #[test]
    fn budget_selects_top_ranked_within_limit() {
        let nodes: Vec<SymbolNode> = (0..5)
            .map(|i| make_node(&format!("f{i}"), 30, 1.0))
            .collect();
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let selected = fit_to_budget(&refs, 100);
        // 3 * 30 = 90 fits, 4 * 30 = 120 exceeds
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn budget_zero_returns_empty() {
        let nodes = vec![make_node("f", 10, 1.0)];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let selected = fit_to_budget(&refs, 0);
        assert!(selected.is_empty());
    }

    #[test]
    fn budget_larger_than_all_returns_all() {
        let nodes: Vec<SymbolNode> = (0..3)
            .map(|i| make_node(&format!("f{i}"), 10, 1.0))
            .collect();
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let selected = fit_to_budget(&refs, 1000);
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn budget_exact_fit_includes_symbol() {
        let nodes = vec![make_node("exact", 50, 1.0)];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let selected = fit_to_budget(&refs, 50);
        assert_eq!(selected.len(), 1);
    }
}
