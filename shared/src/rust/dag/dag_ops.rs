// See shared/src/main/scala/coop/rchain/dag/DagOps.scala

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

pub fn bf_traverse<A, F>(start: Vec<A>, mut neighbors: F) -> Vec<A>
where
    A: Eq + Hash + Clone,
    F: FnMut(&A) -> Vec<A>,
{
    let mut result = Vec::new();
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // Initialize queue with starting nodes
    for node in start {
        queue.push_back(node);
    }

    while let Some(curr) = queue.pop_front() {
        if visited.contains(&curr) {
            continue;
        }

        // Mark as visited and add to result
        visited.insert(curr.clone());
        result.push(curr.clone());

        // Get neighbors and add unvisited ones to queue
        let ns = neighbors(&curr);
        for n in ns {
            if !visited.contains(&n) {
                queue.push_back(n);
            }
        }
    }

    result
}

/// Fallible breadth-first traversal. The `neighbors` closure returns
/// `Result<Vec<A>, E>` so storage errors can be surfaced rather than
/// silently truncating the traversal.
///
/// On the first `Err` the traversal short-circuits and the error is
/// returned. Otherwise the behavior matches `bf_traverse` exactly.
///
/// Use this when the closure performs I/O (storage lookups, network
/// reads, etc.) whose failure would corrupt the result of a consumer
/// — e.g. the consensus snapshot's `deploys_in_scope` set, where a
/// silent shrink could admit duplicate-sig deploys.
pub fn try_bf_traverse<A, E, F>(start: Vec<A>, mut neighbors: F) -> Result<Vec<A>, E>
where
    A: Eq + Hash + Clone,
    F: FnMut(&A) -> Result<Vec<A>, E>,
{
    let mut result = Vec::new();
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    for node in start {
        queue.push_back(node);
    }

    while let Some(curr) = queue.pop_front() {
        if visited.contains(&curr) {
            continue;
        }

        visited.insert(curr.clone());
        result.push(curr.clone());

        let ns = neighbors(&curr)?;
        for n in ns {
            if !visited.contains(&n) {
                queue.push_back(n);
            }
        }
    }

    Ok(result)
}

pub fn bf_traverse_find<A, F, P>(start: Vec<A>, mut neighbors: F, mut predicate: P) -> Option<A>
where
    A: Eq + Hash + Clone,
    F: FnMut(&A) -> Vec<A>,
    P: FnMut(&A) -> bool,
{
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    for node in start {
        queue.push_back(node);
    }

    while let Some(curr) = queue.pop_front() {
        if visited.contains(&curr) {
            continue;
        }

        visited.insert(curr.clone());
        if predicate(&curr) {
            return Some(curr);
        }

        let ns = neighbors(&curr);
        for n in ns {
            if !visited.contains(&n) {
                queue.push_back(n);
            }
        }
    }

    None
}

/// Fallible `bf_traverse_find`: the search stops at the first `Err` instead of
/// treating a failed expansion as "this node has no neighbors".
///
/// Both the expansion and the test are fallible: a search that swallows either
/// cannot distinguish "not found" from "not looked at", and the caller reads the
/// first as a verdict — e.g. the duplicate-deploy scan in
/// `Validate::repeat_deploy`, where an unreadable ancestry would otherwise admit
/// the repeat it exists to reject.
pub fn try_bf_traverse_find<A, E, F, P>(
    start: Vec<A>,
    mut neighbors: F,
    mut predicate: P,
) -> Result<Option<A>, E>
where
    A: Eq + Hash + Clone,
    F: FnMut(&A) -> Result<Vec<A>, E>,
    P: FnMut(&A) -> Result<bool, E>,
{
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    for node in start {
        queue.push_back(node);
    }

    while let Some(curr) = queue.pop_front() {
        if visited.contains(&curr) {
            continue;
        }

        visited.insert(curr.clone());
        if predicate(&curr)? {
            return Ok(Some(curr));
        }

        let ns = neighbors(&curr)?;
        for n in ns {
            if !visited.contains(&n) {
                queue.push_back(n);
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_simple_tree() {
        // Create a simple tree:
        //      1
        //    /   \
        //   2     3
        //  / \   / \
        // 4   5 6   7
        let mut graph = HashMap::new();
        graph.insert(1, vec![2, 3]);
        graph.insert(2, vec![4, 5]);
        graph.insert(3, vec![6, 7]);
        graph.insert(4, vec![]);
        graph.insert(5, vec![]);
        graph.insert(6, vec![]);
        graph.insert(7, vec![]);

        let neighbors = |n: &i32| graph.get(n).unwrap_or(&vec![]).clone();

        let result = bf_traverse(vec![1], neighbors);
        assert_eq!(result, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn test_cyclic_graph() {
        // Create a graph with cycles:
        //  1 --- 2
        //  |     |
        //  3 --- 4
        let mut graph = HashMap::new();
        graph.insert(1, vec![2, 3]);
        graph.insert(2, vec![1, 4]);
        graph.insert(3, vec![1, 4]);
        graph.insert(4, vec![2, 3]);

        let neighbors = |n: &i32| graph.get(n).unwrap_or(&vec![]).clone();

        let result = bf_traverse(vec![1], neighbors);
        // The exact order can vary, but we should visit each node exactly once
        assert_eq!(result.len(), 4);
        assert!(result.contains(&1));
        assert!(result.contains(&2));
        assert!(result.contains(&3));
        assert!(result.contains(&4));
    }

    #[test]
    fn test_empty_start() {
        let neighbors = |_: &i32| vec![];
        let result = bf_traverse(Vec::<i32>::new(), neighbors);
        assert_eq!(result, Vec::<i32>::new());
    }

    #[test]
    fn test_multiple_start_nodes() {
        let mut graph = HashMap::new();
        graph.insert(1, vec![3]);
        graph.insert(2, vec![4]);
        graph.insert(3, vec![]);
        graph.insert(4, vec![]);

        let neighbors = |n: &i32| graph.get(n).unwrap_or(&vec![]).clone();

        let result = bf_traverse(vec![1, 2], neighbors);
        // We should visit all nodes starting with the initial set
        assert_eq!(result.len(), 4);
        // The first two nodes should be our start nodes in order
        assert_eq!(result[0], 1);
        assert_eq!(result[1], 2);
        // The remaining nodes should include 3 and 4 (order may vary)
        assert!(result.contains(&3));
        assert!(result.contains(&4));
    }

    #[test]
    fn test_try_bf_traverse_success() {
        let mut graph: HashMap<i32, Vec<i32>> = HashMap::new();
        graph.insert(1, vec![2, 3]);
        graph.insert(2, vec![4]);
        graph.insert(3, vec![]);
        graph.insert(4, vec![]);

        let neighbors = |n: &i32| -> Result<Vec<i32>, &'static str> {
            Ok(graph.get(n).cloned().unwrap_or_default())
        };
        let result = try_bf_traverse(vec![1], neighbors).expect("traversal must succeed");
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], 1);
    }

    #[test]
    fn test_try_bf_traverse_short_circuits_on_error() {
        let neighbors = |n: &i32| -> Result<Vec<i32>, &'static str> {
            if *n == 2 {
                Err("simulated storage failure")
            } else {
                Ok(vec![2, 3])
            }
        };
        let err = try_bf_traverse(vec![1], neighbors).expect_err("error must propagate");
        assert_eq!(err, "simulated storage failure");
    }

    #[test]
    fn test_bf_traverse_find_stops_on_match() {
        let mut graph = HashMap::new();
        graph.insert(1, vec![2, 3]);
        graph.insert(2, vec![4, 5]);
        graph.insert(3, vec![6, 7]);
        graph.insert(4, vec![]);
        graph.insert(5, vec![]);
        graph.insert(6, vec![]);
        graph.insert(7, vec![]);

        let neighbors = |n: &i32| graph.get(n).unwrap_or(&vec![]).clone();
        let found = bf_traverse_find(vec![1], neighbors, |n| *n == 6);
        assert_eq!(found, Some(6));
    }

    #[test]
    fn test_try_bf_traverse_find_stops_on_match_without_expanding_further() {
        let neighbors = |n: &i32| -> Result<Vec<i32>, &'static str> {
            if *n == 6 {
                Err("a matched node must not be expanded")
            } else {
                Ok(vec![n * 2, n * 3])
            }
        };
        let found = try_bf_traverse_find(vec![1], neighbors, |n| Ok(*n == 6))
            .expect("match must not surface an error");
        assert_eq!(found, Some(6));
    }

    #[test]
    fn test_try_bf_traverse_find_reports_a_failed_test_rather_than_no_match() {
        let neighbors = |n: &i32| -> Result<Vec<i32>, &'static str> { Ok(vec![n * 2]) };
        let predicate = |n: &i32| -> Result<bool, &'static str> {
            if *n == 4 {
                Err("simulated unreadable body")
            } else {
                Ok(false)
            }
        };
        let err = try_bf_traverse_find(vec![1], neighbors, predicate)
            .expect_err("a node that could not be tested must not read as 'no match'");
        assert_eq!(err, "simulated unreadable body");
    }

    #[test]
    fn test_try_bf_traverse_find_reports_the_error_rather_than_not_found() {
        let neighbors = |n: &i32| -> Result<Vec<i32>, &'static str> {
            if *n == 2 {
                Err("simulated storage failure")
            } else {
                Ok(vec![2, 3])
            }
        };
        let err = try_bf_traverse_find(vec![1], neighbors, |n| Ok(*n == 99))
            .expect_err("an unreadable branch must not read as 'not found'");
        assert_eq!(err, "simulated storage failure");
    }
}
