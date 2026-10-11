//! The stable merge: when results change while the list is on screen
//! without the user typing (apps finishing their scan, a slow provider
//! answering), rows don't move under the selection.
//!
//! DESIGN.md's rule: once a row has been on screen, later results are
//! inserted only below the selection, never above it; a new keystroke
//! re-ranks everything. So the rows from the top to the selected one keep
//! their places (those that still exist), and everything else follows in
//! the new ranking.

use std::collections::HashMap;

use crate::index::Hit;

/// Merges `fresh` results into a list showing `shown` (ids, top first)
/// with row `selected` selected.
pub fn stable_merge(shown: &[String], selected: usize, fresh: Vec<Hit>) -> Vec<Hit> {
    let keep = shown.len().min(selected.saturating_add(1));
    let mut by_id: HashMap<String, Hit> = HashMap::with_capacity(fresh.len());
    let mut order = Vec::with_capacity(fresh.len());
    for hit in fresh {
        order.push(hit.id.clone());
        by_id.insert(hit.id.clone(), hit);
    }
    let mut merged: Vec<Hit> = shown[..keep]
        .iter()
        .filter_map(|id| by_id.remove(id))
        .collect();
    merged.extend(order.into_iter().filter_map(|id| by_id.remove(&id)));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(ids: &[&str]) -> Vec<Hit> {
        ids.iter()
            .map(|id| Hit {
                id: (*id).into(),
                score: 0.0,
                highlights: vec![],
            })
            .collect()
    }

    fn shown(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    #[test]
    fn rows_down_to_the_selection_stay_and_new_ones_go_below() {
        let merged = stable_merge(&shown(&["b", "c", "d"]), 1, hits(&["new", "c", "b", "d"]));
        assert_eq!(ids(&merged), ["b", "c", "new", "d"]);
    }

    #[test]
    fn a_kept_row_that_disappeared_is_dropped() {
        let merged = stable_merge(&shown(&["b", "gone", "d"]), 2, hits(&["x", "d", "b"]));
        assert_eq!(ids(&merged), ["b", "d", "x"]);
    }

    #[test]
    fn with_nothing_shown_the_new_ranking_stands() {
        assert_eq!(ids(&stable_merge(&[], 0, hits(&["x", "y"]))), ["x", "y"]);
    }

    #[test]
    fn rows_below_the_selection_may_reorder() {
        let merged = stable_merge(&shown(&["a", "b", "c"]), 0, hits(&["c", "b", "a"]));
        assert_eq!(ids(&merged), ["a", "c", "b"]);
    }
}
