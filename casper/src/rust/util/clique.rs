use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use shared::rust::dag::observation_work::{NoopWork, WorkKind, WorkMeter};
use shared::rust::store::key_value_store::KvStoreError;

pub struct Clique;

impl Clique {
    pub fn find_maximum_clique_by_weight<A>(edges: &[(A, A)], weights: &HashMap<A, i64>) -> i64
    where A: Eq + Hash + Clone + Ord {
        Self::find_maximum_clique_by_weight_metered(&NoopWork, edges, weights)
            .expect("The no-op work meter cannot reject clique search")
    }

    pub fn find_maximum_clique_by_weight_metered<A, W>(
        meter: &W,
        edges: &[(A, A)],
        weights: &HashMap<A, i64>,
    ) -> Result<i64, KvStoreError>
    where
        A: Eq + Hash + Clone + Ord,
        W: WorkMeter,
    {
        let mut adj: HashMap<A, HashSet<A>> = HashMap::new();
        let mut nodes = HashSet::new();
        for (a, b) in edges {
            meter.charge(WorkKind::Clique, 1, 0)?;
            meter.allocate(8, std::mem::size_of::<A>() + 128)?;
            nodes.insert(a.clone());
            nodes.insert(b.clone());
            if a != b {
                adj.entry(a.clone()).or_default().insert(b.clone());
                adj.entry(b.clone()).or_default().insert(a.clone());
            }
        }
        let mut best_weight = None;
        for weight in weights.values() {
            meter.step(WorkKind::Clique)?;
            best_weight = Some(best_weight.map_or(*weight, |best: i64| best.max(*weight)));
        }
        let mut best_weight = best_weight.unwrap_or(0);
        Self::expand_max_weight(
            meter,
            0,
            0,
            &nodes,
            &HashSet::new(),
            &adj,
            weights,
            &mut best_weight,
        )?;
        Ok(best_weight)
    }

    fn sum<W: WorkMeter>(left: i64, right: i64) -> Result<i64, KvStoreError> {
        if W::ENABLED {
            left.checked_add(right).ok_or_else(|| {
                KvStoreError::InvalidArgument("observation_work:stake_overflow".to_string())
            })
        } else {
            Ok(left + right)
        }
    }

    fn copy_set<A: Eq + Hash + Clone, W: WorkMeter>(
        meter: &W,
        source: &HashSet<A>,
    ) -> Result<HashSet<A>, KvStoreError> {
        meter.allocate(source.len(), std::mem::size_of::<A>() * 2 + 128)?;
        let mut result = HashSet::new();
        for value in source {
            meter.step(WorkKind::Clique)?;
            result.insert(value.clone());
        }
        Ok(result)
    }

    fn intersection<A: Eq + Hash + Clone, W: WorkMeter>(
        meter: &W,
        left: &HashSet<A>,
        right: &HashSet<A>,
    ) -> Result<HashSet<A>, KvStoreError> {
        meter.allocate(
            left.len().min(right.len()),
            std::mem::size_of::<A>() * 2 + 128,
        )?;
        let mut result = HashSet::new();
        let (small, large) = if left.len() <= right.len() {
            (left, right)
        } else {
            (right, left)
        };
        for value in small {
            meter.step(WorkKind::Clique)?;
            if large.contains(value) {
                result.insert(value.clone());
            }
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_max_weight<A, W>(
        meter: &W,
        depth: usize,
        ans_weight: i64,
        p: &HashSet<A>,
        x: &HashSet<A>,
        adj: &HashMap<A, HashSet<A>>,
        weights: &HashMap<A, i64>,
        best_weight: &mut i64,
    ) -> Result<(), KvStoreError>
    where
        A: Eq + Hash + Clone + Ord,
        W: WorkMeter,
    {
        meter.expand(depth)?;
        if p.is_empty() && x.is_empty() {
            if depth > 0 && ans_weight > *best_weight {
                *best_weight = ans_weight;
            }
            return Ok(());
        }
        if p.is_empty() {
            return Ok(());
        }
        let mut remaining_weight = 0;
        for vertex in p {
            meter.step(WorkKind::Clique)?;
            remaining_weight =
                Self::sum::<W>(remaining_weight, *weights.get(vertex).unwrap_or(&0))?;
        }
        if Self::sum::<W>(ans_weight, remaining_weight)? <= *best_weight {
            return Ok(());
        }
        let empty = HashSet::new();
        let mut pivot = None;
        let mut pivot_count = 0;
        for vertex in p.iter().chain(x.iter()) {
            meter.step(WorkKind::Clique)?;
            let neighbors = adj.get(vertex).unwrap_or(&empty);
            let mut count = 0;
            for candidate in p {
                meter.step(WorkKind::Clique)?;
                count += usize::from(neighbors.contains(candidate));
            }
            if pivot.is_none() || count >= pivot_count {
                pivot = Some(vertex);
                pivot_count = count;
            }
        }
        let pivot_neighbors = adj
            .get(pivot.expect("Nonempty candidates contain a pivot"))
            .unwrap_or(&empty);
        meter.allocate(p.len(), std::mem::size_of::<A>() * 2 + 128)?;
        let mut candidates = Vec::new();
        for vertex in p {
            meter.step(WorkKind::Clique)?;
            if !pivot_neighbors.contains(vertex) {
                candidates.push(vertex.clone());
            }
        }
        let mut current_p = Self::copy_set(meter, p)?;
        let mut current_x = Self::copy_set(meter, x)?;
        for vertex in candidates {
            meter.step(WorkKind::Clique)?;
            let neighbors = adj.get(&vertex).unwrap_or(&empty);
            let new_p = Self::intersection(meter, &current_p, neighbors)?;
            let new_x = Self::intersection(meter, &current_x, neighbors)?;
            Self::expand_max_weight(
                meter,
                depth + 1,
                Self::sum::<W>(ans_weight, *weights.get(&vertex).unwrap_or(&0))?,
                &new_p,
                &new_x,
                adj,
                weights,
                best_weight,
            )?;
            current_p.remove(&vertex);
            meter.allocate(1, std::mem::size_of::<A>() * 2 + 128)?;
            current_x.insert(vertex);
        }
        Ok(())
    }
}
