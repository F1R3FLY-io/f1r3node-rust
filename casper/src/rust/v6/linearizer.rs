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
    emitted: &HashSet<BlockHash>,
) -> Result<Vec<BlockHash>, KvStoreError> {
    if emitted.contains(tip) {
        return Ok(Vec::new());
    }
    let mut members: HashSet<BlockHash> = HashSet::new();
    let mut parents_of: HashMap<BlockHash, Vec<BlockHash>> = HashMap::new();
    let mut queue = VecDeque::from([tip.clone()]);
    while let Some(hash) = queue.pop_front() {
        if emitted.contains(&hash) || !members.insert(hash.clone()) {
            continue;
        }
        let parents = dag.parents(&hash)?;
        for parent in &parents {
            if !emitted.contains(parent) && !members.contains(parent) {
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

pub struct Linearizer {
    last: BlockHash,
    last_number: i64,
    emitted: HashSet<BlockHash>,
}

impl Linearizer {
    pub fn new(genesis: BlockHash, genesis_number: i64) -> Self {
        Linearizer {
            emitted: HashSet::from([genesis.clone()]),
            last: genesis,
            last_number: genesis_number,
        }
    }

    pub fn last(&self) -> &BlockHash {
        &self.last
    }

    pub fn advance<D: DagView>(
        &mut self,
        dag: &D,
        lfb: &BlockHash,
    ) -> Result<Advance, KvStoreError> {
        if *lfb == self.last {
            return Ok(Advance::Unchanged);
        }
        let mut chain = Vec::new();
        let mut cursor = lfb.clone();
        loop {
            if cursor == self.last {
                break;
            }
            if dag.number(&cursor)? <= self.last_number {
                return Ok(Advance::Hold {
                    previous: self.last.clone(),
                    proposed: lfb.clone(),
                });
            }
            chain.push(cursor.clone());
            match dag.parents(&cursor)?.first() {
                Some(main_parent) => cursor = main_parent.clone(),
                None => {
                    return Ok(Advance::Hold {
                        previous: self.last.clone(),
                        proposed: lfb.clone(),
                    })
                }
            }
        }
        let mut out = Vec::new();
        for main in chain.into_iter().rev() {
            let epoch = epoch_order(dag, &main, &self.emitted)?;
            self.emitted.extend(epoch.iter().cloned());
            out.extend(epoch);
        }
        self.last_number = dag.number(lfb)?;
        self.last = lfb.clone();
        Ok(Advance::Emitted(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::bytes::Bytes;

    struct TestDag(HashMap<BlockHash, (i64, Vec<BlockHash>)>);

    impl DagView for TestDag {
        fn parents(&self, hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError> {
            Ok(self.0[hash].1.clone())
        }
        fn number(&self, hash: &BlockHash) -> Result<i64, KvStoreError> {
            Ok(self.0[hash].0)
        }
    }

    fn h(s: &str) -> BlockHash {
        Bytes::copy_from_slice(s.as_bytes())
    }

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
        let mut lin = Linearizer::new(h("g"), 0);
        let Advance::Emitted(out) = lin.advance(&d, &h("m3")).unwrap() else {
            panic!()
        };
        assert_eq!(names(&out), vec!["a", "x", "m1", "s1", "m2", "s2", "m3"]);
    }

    #[test]
    fn order_is_independent_of_finality_steps() {
        let d = sample();
        let mut one = Linearizer::new(h("g"), 0);
        let Advance::Emitted(all) = one.advance(&d, &h("m3")).unwrap() else {
            panic!()
        };
        let mut two = Linearizer::new(h("g"), 0);
        let mut stepped = Vec::new();
        for lfb in ["m1", "m2", "m3"] {
            let Advance::Emitted(part) = two.advance(&d, &h(lfb)).unwrap() else {
                panic!()
            };
            stepped.extend(part);
        }
        assert_eq!(all, stepped);
    }

    #[test]
    fn every_block_appears_once_and_after_its_parents() {
        let d = sample();
        let mut lin = Linearizer::new(h("g"), 0);
        let Advance::Emitted(out) = lin.advance(&d, &h("m3")).unwrap() else {
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
        let mut lin = Linearizer::new(h("g"), 0);
        lin.advance(&d, &h("a")).unwrap();
        assert_eq!(
            lin.advance(&d, &h("c")).unwrap(),
            Advance::Hold {
                previous: h("a"),
                proposed: h("c")
            }
        );
    }

    #[test]
    fn unchanged_lfb_emits_nothing() {
        let d = sample();
        let mut lin = Linearizer::new(h("g"), 0);
        lin.advance(&d, &h("m2")).unwrap();
        assert_eq!(lin.advance(&d, &h("m2")).unwrap(), Advance::Unchanged);
    }
}
