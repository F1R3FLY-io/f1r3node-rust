use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use num_bigint::BigInt;
use shared::rust::dag::observation_work::{NoopWork, WorkKind, WorkMeter};
use shared::rust::store::key_value_store::KvStoreError;

pub struct Clique;

impl Clique {
    pub fn find_maximum_clique_by_weight<A>(
        edges: &[(A, A)],
        weights: &HashMap<A, BigInt>,
    ) -> BigInt
    where
        A: Eq + Hash + Clone + Ord,
    {
        Self::find_maximum_clique_by_weight_metered(&NoopWork, edges, weights)
            .expect("The no-op work meter cannot reject clique search")
    }

    pub fn find_maximum_clique_by_weight_metered<A, W>(
        meter: &W,
        edges: &[(A, A)],
        weights: &HashMap<A, BigInt>,
    ) -> Result<BigInt, KvStoreError>
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
            best_weight =
                Some(best_weight.map_or(weight.clone(), |best: BigInt| best.max(weight.clone())));
        }
        let mut best_weight = best_weight.unwrap_or_default();
        Self::expand_max_weight(
            meter,
            0,
            BigInt::from(0),
            &nodes,
            &HashSet::new(),
            &adj,
            weights,
            &mut best_weight,
        )?;
        Ok(best_weight)
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
        ans_weight: BigInt,
        p: &HashSet<A>,
        x: &HashSet<A>,
        adj: &HashMap<A, HashSet<A>>,
        weights: &HashMap<A, BigInt>,
        best_weight: &mut BigInt,
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
        let mut remaining_weight = BigInt::from(0);
        for vertex in p {
            meter.step(WorkKind::Clique)?;
            if let Some(weight) = weights.get(vertex) {
                remaining_weight += weight;
            }
        }
        if &ans_weight + remaining_weight <= *best_weight {
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
                &ans_weight + weights.get(&vertex).cloned().unwrap_or_default(),
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
