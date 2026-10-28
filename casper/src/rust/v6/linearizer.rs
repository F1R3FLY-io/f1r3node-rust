use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use shared::rust::store::key_value_store::KvStoreError;

pub trait DagView {
    fn parents(&self, hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError>;
    fn number(&self, hash: &BlockHash) -> Result<i64, KvStoreError>;
}

impl DagView for KeyValueDagRepresentation {
    fn parents(&self, hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError> {
        Ok(self.lookup_unsafe(hash)?.parents)
    }

    fn number(&self, hash: &BlockHash) -> Result<i64, KvStoreError> {
        Ok(self.lookup_unsafe(hash)?.block_number)
    }
}

pub fn epoch_order<D: DagView>(
    dag: &D,
    tip: &BlockHash,
    emitted: &dyn Fn(&BlockHash) -> bool,
) -> Result<Vec<BlockHash>, KvStoreError> {
    if emitted(tip) {
        return Ok(Vec::new());
    }
    let mut members: HashSet<BlockHash> = HashSet::new();
    let mut parents_of: HashMap<BlockHash, Vec<BlockHash>> = HashMap::new();
    let mut queue = VecDeque::from([tip.clone()]);
    while let Some(hash) = queue.pop_front() {
        if emitted(&hash) || !members.insert(hash.clone()) {
            continue;
        }
        let parents = dag.parents(&hash)?;
        for parent in &parents {
            if !members.contains(parent) && !emitted(parent) {
                queue.push_back(parent.clone());
            }
        }
        parents_of.insert(hash, parents);
    }

    let mut pending: HashMap<BlockHash, usize> = HashMap::new();
    let mut children: HashMap<BlockHash, Vec<BlockHash>> = HashMap::new();
    for (hash, parents) in &parents_of {
        let inside: Vec<&BlockHash> = parents.iter().filter(|p| members.contains(*p)).collect();
        pending.insert(hash.clone(), inside.len());
        for parent in inside {
            children
                .entry(parent.clone())
                .or_default()
                .push(hash.clone());
        }
    }

    let mut ready: BTreeSet<(i64, BlockHash)> = BTreeSet::new();
    for (hash, count) in &pending {
        if *count == 0 {
            ready.insert((dag.number(hash)?, hash.clone()));
        }
    }
    let mut out = Vec::with_capacity(members.len());
    while let Some(next) = ready.pop_first() {
        let hash = next.1;
        if let Some(kids) = children.get(&hash) {
            for kid in kids {
                let count = pending.get_mut(kid).expect("child is a member");
                *count -= 1;
                if *count == 0 {
                    ready.insert((dag.number(kid)?, kid.clone()));
                }
            }
        }
        out.push(hash);
    }
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Advance {
    Unchanged,
    Emitted(Vec<BlockHash>),
    Hold {
        previous: BlockHash,
        proposed: BlockHash,
    },
}

#[derive(Default)]
pub struct Linearizer {
    last: Option<(BlockHash, i64)>,
}

impl Linearizer {
    pub fn new() -> Self { Linearizer { last: None } }

    pub fn last(&self) -> Option<&BlockHash> { self.last.as_ref().map(|(hash, _)| hash) }

    pub fn advance<D: DagView>(
        &mut self,
        dag: &D,
        lfb: &BlockHash,
        emitted: &dyn Fn(&BlockHash) -> bool,
    ) -> Result<Advance, KvStoreError> {
        if self.last.as_ref().is_some_and(|(last, _)| last == lfb) {
            return Ok(Advance::Unchanged);
        }
        let hold = |last: &BlockHash| Advance::Hold {
            previous: last.clone(),
            proposed: lfb.clone(),
        };
        let mut chain = Vec::new();
        let mut cursor = lfb.clone();
        loop {
            if emitted(&cursor) {
                match &self.last {
                    Some((last, _)) if *last != cursor => return Ok(hold(last)),
                    _ => break,
                }
            }
            if let Some((last, last_number)) = &self.last {
                if dag.number(&cursor)? <= *last_number {
                    return Ok(hold(last));
                }
            }
            chain.push(cursor.clone());
            match dag.parents(&cursor)?.first() {
                Some(main_parent) => cursor = main_parent.clone(),
                None => {
                    return Ok(match &self.last {
                        Some((last, _)) => hold(last),
                        None => Advance::Hold {
                            previous: cursor,
                            proposed: lfb.clone(),
                        },
                    })
                }
            }
        }
        let mut fresh: HashSet<BlockHash> = HashSet::new();
        let mut out = Vec::new();
        for main in chain.into_iter().rev() {
            let epoch = {
                let seen = |hash: &BlockHash| fresh.contains(hash) || emitted(hash);
                epoch_order(dag, &main, &seen)?
            };
            fresh.extend(epoch.iter().cloned());
            out.extend(epoch);
        }
        self.last = Some((lfb.clone(), dag.number(lfb)?));
        Ok(Advance::Emitted(out))
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    struct TestDag(HashMap<BlockHash, (i64, Vec<BlockHash>)>);

    impl DagView for TestDag {
        fn parents(&self, hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError> {
            Ok(self.0[hash].1.clone())
        }
        fn number(&self, hash: &BlockHash) -> Result<i64, KvStoreError> { Ok(self.0[hash].0) }
    }

    fn h(s: &str) -> BlockHash { Bytes::copy_from_slice(s.as_bytes()) }

    fn dag(edges: &[(&str, i64, &[&str])]) -> TestDag {
        TestDag(
            edges
                .iter()
                .map(|(n, num, ps)| (h(n), (*num, ps.iter().map(|p| h(p)).collect())))
                .collect(),
        )
    }

    fn names(v: &[BlockHash]) -> Vec<String> {
        v.iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .collect()
    }

    struct Run {
        lin: Linearizer,
        emitted: HashSet<BlockHash>,
    }

    impl Run {
        fn new() -> Self { Run::from(&["g"]) }

        fn from(emitted: &[&str]) -> Self {
            Run {
                lin: Linearizer::new(),
                emitted: emitted.iter().map(|b| h(b)).collect(),
            }
        }

        fn advance(&mut self, d: &TestDag, lfb: &str) -> Advance {
            let emitted = self.emitted.clone();
            let advance = self
                .lin
                .advance(d, &h(lfb), &|b| emitted.contains(b))
                .unwrap();
            if let Advance::Emitted(out) = &advance {
                self.emitted.extend(out.iter().cloned());
            }
            advance
        }
    }

    fn sample() -> TestDag {
        dag(&[
            ("g", 0, &[]),
            ("a", 1, &["g"]),
            ("x", 1, &["g"]),
            ("m1", 2, &["a", "x"]),
            ("s1", 2, &["a"]),
            ("m2", 3, &["m1", "s1"]),
            ("s2", 3, &["m1"]),
            ("m3", 4, &["m2", "s2"]),
        ])
    }

    #[test]
    fn epochs_follow_the_main_chain_and_respect_ancestry() {
        let d = sample();
        let mut lin = Run::new();
        let Advance::Emitted(out) = lin.advance(&d, "m3") else {
            panic!()
        };
        assert_eq!(names(&out), vec!["a", "x", "m1", "s1", "m2", "s2", "m3"]);
    }

    #[test]
    fn order_is_independent_of_finality_steps() {
        let d = sample();
        let mut one = Run::new();
        let Advance::Emitted(all) = one.advance(&d, "m3") else {
            panic!()
        };
        let mut two = Run::new();
        let mut stepped = Vec::new();
        for lfb in ["m1", "m2", "m3"] {
            let Advance::Emitted(part) = two.advance(&d, lfb) else {
                panic!()
            };
            stepped.extend(part);
        }
        assert_eq!(all, stepped);
    }

    #[test]
    fn every_block_appears_once_and_after_its_parents() {
        let d = sample();
        let mut lin = Run::new();
        let Advance::Emitted(out) = lin.advance(&d, "m3") else {
            panic!()
        };
        let position: HashMap<&BlockHash, usize> =
            out.iter().enumerate().map(|(i, b)| (b, i)).collect();
        assert_eq!(position.len(), out.len());
        for block in &out {
            for parent in d.parents(block).unwrap() {
                if parent != h("g") {
                    assert!(position[&parent] < position[block]);
                }
            }
        }
    }

    #[test]
    fn an_lfb_off_the_previous_main_chain_holds() {
        let d = dag(&[
            ("g", 0, &[]),
            ("a", 1, &["g"]),
            ("b", 1, &["g"]),
            ("c", 2, &["b", "a"]),
        ]);
        let mut lin = Run::new();
        lin.advance(&d, "a");
        assert_eq!(lin.advance(&d, "c"), Advance::Hold {
            previous: h("a"),
            proposed: h("c")
        });
    }

    #[test]
    fn unchanged_lfb_emits_nothing() {
        let d = sample();
        let mut lin = Run::new();
        lin.advance(&d, "m2");
        assert_eq!(lin.advance(&d, "m2"), Advance::Unchanged);
    }

    #[test]
    fn a_resumed_linearizer_emits_the_same_tail_after_any_canonical_prefix() {
        let d = sample();
        let mut full = Run::new();
        let Advance::Emitted(all) = full.advance(&d, "m3") else {
            panic!()
        };
        for cut in 1..all.len() {
            let mut prefix: Vec<&str> = vec!["g"];
            let names = names(&all[..cut]);
            prefix.extend(names.iter().map(String::as_str));
            let mut resumed = Run::from(&prefix);
            let Advance::Emitted(tail) = resumed.advance(&d, "m3") else {
                panic!("cut {cut}")
            };
            assert_eq!(tail, all[cut..].to_vec(), "cut {cut}");
        }
    }

    #[test]
    fn a_resumed_linearizer_checks_a_lin_from_its_first_step_on() {
        let d = dag(&[
            ("g", 0, &[]),
            ("a", 1, &["g"]),
            ("b", 1, &["g"]),
            ("c", 2, &["b", "a"]),
        ]);
        let mut resumed = Run::from(&["g", "a"]);
        assert!(matches!(resumed.advance(&d, "a"), Advance::Emitted(out) if out.is_empty()));
        assert_eq!(resumed.advance(&d, "c"), Advance::Hold {
            previous: h("a"),
            proposed: h("c")
        });
    }
}
