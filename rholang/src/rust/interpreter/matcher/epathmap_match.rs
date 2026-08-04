//! Trie-native spatial matching for homogeneous EPathMaps.
//!
//! The ordinary set/map matcher projects both sides into vectors before a
//! bipartite search. EPathMap deliberately does not: exact entries are resolved
//! by encoded-key lookup and PathMap subtraction, while connective patterns are
//! matched by an explicit augmenting-path PDA whose assignments and seen set
//! are PathMaps over the target's canonical keys. Prefix compression therefore
//! survives the entire operation.

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EPathMap, Expr, Par, Var};
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::utils::{isolate_free_map, FreeMap};

use super::list_match::aggregate_updates;
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};

#[derive(Clone)]
enum PathPattern {
    Remainder,
    Set(Arc<Par>),
    Map(Arc<(Par, Par)>),
}

#[derive(Clone)]
struct Assignment {
    pattern: PathPattern,
    free_map: FreeMap,
}

struct SearchFrame {
    pattern: PathPattern,
    next_target: Vec<u8>,
    pending: Option<(Vec<u8>, FreeMap)>,
}

#[derive(Clone, Copy)]
enum Remainder {
    None,
    Wildcard,
    Free(i32),
}

impl Remainder {
    fn from_var(remainder: &Option<Var>) -> Self {
        match remainder.as_ref().and_then(|var| var.var_instance.as_ref()) {
            Some(VarInstance::Wildcard(_)) => Remainder::Wildcard,
            Some(VarInstance::FreeVar(level)) => Remainder::Free(*level),
            _ => Remainder::None,
        }
    }
}

impl SpatialMatcherContext {
    pub(super) fn spatial_match_epathmap(
        &mut self,
        target: EPathMap,
        pattern: EPathMap,
    ) -> Option<()> {
        let target_mode = target.mode();
        let pattern_mode = pattern.mode();
        if pattern_mode != EPathMapMode::Empty && target_mode != pattern_mode {
            return None;
        }

        let remainder = Remainder::from_var(&pattern.remainder);
        let (remaining, dynamic_patterns) = prepare_exact_entries(&target, &pattern)?;
        let target_count = remaining.len();

        match remainder {
            Remainder::None if dynamic_patterns != target_count => return None,
            Remainder::Wildcard | Remainder::Free(_) if dynamic_patterns > target_count => {
                return None;
            }
            _ => {}
        }

        let mut assignments = pathmap::PathMap::<Assignment>::new();

        // Match remainders first, exactly as list_match does. Later explicit
        // patterns may displace them, which is what ensures a non-concrete
        // target is assigned to an explicit pattern whenever that is possible.
        if matches!(remainder, Remainder::Free(_)) {
            for _ in 0..(target_count - dynamic_patterns) {
                if !self.augment_pathmap(PathPattern::Remainder, &remaining, &mut assignments) {
                    return None;
                }
            }
        }

        let mut pattern_key = Vec::new();
        while let Some((key, dynamic)) = next_dynamic_pattern(&pattern, &pattern_key) {
            pattern_key = key;
            if !self.augment_pathmap(dynamic, &remaining, &mut assignments) {
                return None;
            }
        }

        let mut remainder_map = EPathMap::default();
        let mut free_maps = Vec::with_capacity(assignments.val_count());
        for (target_key, assignment) in assignments {
            if matches!(assignment.pattern, PathPattern::Remainder) {
                match remaining.mode() {
                    EPathMapMode::Empty => {}
                    EPathMapMode::Set => {
                        let member = decode_trie_path(&target_key)
                            .expect("set-mode EPathMap keys are canonical Par paths");
                        remainder_map.insert_entry(member);
                    }
                    EPathMapMode::Map => {
                        let key = decode_trie_path(&target_key)
                            .expect("map-mode EPathMap keys are canonical Par paths");
                        let value = remaining
                            .entry_trie()
                            .get_map_value_by_encoded_key(&target_key)
                            .expect("remaining target retains map mode")
                            .expect("assignment keys name target values")
                            .clone();
                        remainder_map
                            .insert_map_entry(key, value)
                            .expect("a fresh remainder cannot mix homogeneous modes");
                    }
                }
            }
            free_maps.push(assignment.free_map);
        }

        self.free_map = aggregate_updates(self.free_map.clone(), free_maps)?;
        if let Remainder::Free(level) = remainder {
            let bound = self
                .free_map
                .get(&level)
                .cloned()
                .unwrap_or_default()
                .with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(remainder_map)),
                }]);
            self.free_map.insert(level, bound);
        }
        Some(())
    }

    fn augment_pathmap(
        &mut self,
        root: PathPattern,
        targets: &EPathMap,
        assignments: &mut pathmap::PathMap<Assignment>,
    ) -> bool {
        let mut seen = pathmap::PathMap::<()>::new();
        let mut stack = vec![SearchFrame {
            pattern: root,
            next_target: Vec::new(),
            pending: None,
        }];

        loop {
            let Some(frame) = stack.last_mut() else {
                return false;
            };
            let mut descended = false;

            while let Some(target_key) = targets.next_value_key(&frame.next_target) {
                frame.next_target = target_key.clone();
                if seen.contains(&target_key) {
                    continue;
                }
                let Some(free_map) = self.match_path_edge(&frame.pattern, targets, &target_key)
                else {
                    continue;
                };
                seen.insert(&target_key, ());

                match assignments
                    .get(&target_key)
                    .map(|assignment| assignment.pattern.clone())
                {
                    None => {
                        assignments.insert(&target_key, Assignment {
                            pattern: frame.pattern.clone(),
                            free_map,
                        });
                        stack.pop();
                        while let Some(mut parent) = stack.pop() {
                            let (parent_target, parent_free_map) = parent
                                .pending
                                .take()
                                .expect("an augmenting parent has one displaced edge");
                            assignments.insert(&parent_target, Assignment {
                                pattern: parent.pattern,
                                free_map: parent_free_map,
                            });
                        }
                        return true;
                    }
                    Some(displaced) => {
                        frame.pending = Some((target_key, free_map));
                        stack.push(SearchFrame {
                            pattern: displaced,
                            next_target: Vec::new(),
                            pending: None,
                        });
                        descended = true;
                        break;
                    }
                }
            }

            if descended {
                continue;
            }
            stack.pop();
            if let Some(parent) = stack.last_mut() {
                parent.pending = None;
            } else {
                return false;
            }
        }
    }

    fn match_path_edge(
        &mut self,
        pattern: &PathPattern,
        targets: &EPathMap,
        target_key: &[u8],
    ) -> Option<FreeMap> {
        match pattern {
            PathPattern::Remainder => {
                let concrete = match targets.mode() {
                    EPathMapMode::Empty => false,
                    EPathMapMode::Set => decode_trie_path(target_key)
                        .expect("set-mode EPathMap keys are canonical Par paths")
                        .locally_free
                        .is_empty(),
                    EPathMapMode::Map => {
                        let key = decode_trie_path(target_key)
                            .expect("map-mode EPathMap keys are canonical Par paths");
                        let value = targets
                            .entry_trie()
                            .get_map_value_by_encoded_key(target_key)
                            .expect("target mode checked")
                            .expect("candidate key names a value");
                        key.locally_free.is_empty() && value.locally_free.is_empty()
                    }
                };
                concrete.then(|| self.free_map.clone())
            }
            PathPattern::Set(pattern) => {
                let target = decode_trie_path(target_key)
                    .expect("set-mode EPathMap keys are canonical Par paths");
                let pattern = pattern.as_ref().clone();
                let (effect, produced) =
                    isolate_free_map(self, |matcher| matcher.spatial_match(target, pattern));
                effect.map(|_| produced)
            }
            PathPattern::Map(pattern) => {
                let target_key_par = decode_trie_path(target_key)
                    .expect("map-mode EPathMap keys are canonical Par paths");
                let target_value = targets
                    .entry_trie()
                    .get_map_value_by_encoded_key(target_key)
                    .expect("target mode checked")
                    .expect("candidate key names a value")
                    .clone();
                let pattern = pattern.as_ref().clone();
                let (effect, produced) = isolate_free_map(self, |matcher| {
                    matcher.spatial_match((target_key_par, target_value), pattern)
                });
                effect.map(|_| produced)
            }
        }
    }
}

fn prepare_exact_entries(target: &EPathMap, pattern: &EPathMap) -> Option<(EPathMap, usize)> {
    let mut exact_mask = EPathMap::default();
    let mut dynamic = 0usize;
    let mut valid = true;

    match pattern.mode() {
        EPathMapMode::Empty => {}
        EPathMapMode::Set => {
            pattern
                .entry_trie()
                .for_each_raw_set_entry(|encoded| {
                    let entry = decode_trie_path(encoded)
                        .expect("set-mode EPathMap keys are canonical Par paths");
                    if entry.connective_used {
                        dynamic += 1;
                    } else {
                        valid &= target.contains_encoded_key(encoded);
                        exact_mask.insert_entry(entry);
                    }
                })
                .expect("pattern mode checked");
        }
        EPathMapMode::Map => {
            pattern
                .entry_trie()
                .for_each_raw_map_entry(|encoded, pattern_value| {
                    let key = decode_trie_path(encoded)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    if key.connective_used || pattern_value.connective_used {
                        dynamic += 1;
                    } else {
                        let target_value = target
                            .entry_trie()
                            .get_map_value_by_encoded_key(encoded)
                            .ok()
                            .flatten();
                        valid &= target_value == Some(pattern_value);
                        exact_mask
                            .insert_map_entry(key, Par::default())
                            .expect("an exact mask cannot mix homogeneous modes");
                    }
                })
                .expect("pattern mode checked");
        }
    }

    if !valid {
        return None;
    }
    let remaining_entries = target
        .entry_trie()
        .try_subtract(exact_mask.entry_trie())
        .ok()?;
    Some((
        EPathMap::new(
            remaining_entries,
            target.locally_free.clone(),
            target.connective_used,
            target.remainder.clone(),
        ),
        dynamic,
    ))
}

fn next_dynamic_pattern(pattern: &EPathMap, after: &[u8]) -> Option<(Vec<u8>, PathPattern)> {
    let mut cursor = after.to_vec();
    loop {
        let key = pattern.next_value_key(&cursor)?;
        cursor = key.clone();
        let decoded = decode_trie_path(&key).expect("EPathMap keys are canonical Par paths");
        match pattern.mode() {
            EPathMapMode::Empty => return None,
            EPathMapMode::Set if decoded.connective_used => {
                return Some((key, PathPattern::Set(Arc::new(decoded))));
            }
            EPathMapMode::Set => {}
            EPathMapMode::Map => {
                let value = pattern
                    .entry_trie()
                    .get_map_value_by_encoded_key(&key)
                    .expect("pattern mode checked")
                    .expect("pattern key names a value");
                if decoded.connective_used || value.connective_used {
                    return Some((key, PathPattern::Map(Arc::new((decoded, value.clone())))));
                }
            }
        }
    }
}
