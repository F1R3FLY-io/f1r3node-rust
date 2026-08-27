use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;

use super::support::encode;
use crate::rspace::persistence::{
    CanonicalDagNode, DagChild, EndpointValue, MutableObjectStore, ObjectAdmission, ObjectRefV1,
};

pub(super) type Entries = BTreeMap<Vec<u8>, EndpointValue>;

#[derive(Debug)]
enum RebuildWork {
    Enter {
        start: usize,
        end: usize,
        depth: usize,
    },
    Assemble {
        prefix: Vec<u8>,
        endpoint: EndpointValue,
        child_edges: Vec<u8>,
    },
}

/// Independent, stack-safe canonical radix-DAG rebuild oracle.
///
/// This deliberately does not call the production differential editor or
/// mirror its cursor representation. It partitions an ordered logical map by
/// maximal common prefixes and uses an explicit post-order work machine, so a
/// deep-path test never replaces production recursion with test recursion.
pub(super) fn rebuild<S>(store: &S, entries: &Entries) -> ObjectRefV1
where
    S: MutableObjectStore,
    S::Error: Debug,
{
    if entries.is_empty() {
        return admit_node(store, &CanonicalDagNode::empty());
    }

    let ordered = entries.iter().collect::<Vec<_>>();
    let mut work = vec![RebuildWork::Enter {
        start: 0,
        end: ordered.len(),
        depth: 0,
    }];
    let mut completed = Vec::new();

    while let Some(step) = work.pop() {
        match step {
            RebuildWork::Enter { start, end, depth } => {
                let partition = &ordered[start..end];
                let first = partition
                    .first()
                    .expect("an oracle partition is nonempty")
                    .0;
                let last = partition.last().expect("an oracle partition is nonempty").0;
                let mut shared = depth;
                let limit = first.len().min(last.len());
                while shared < limit && first[shared] == last[shared] {
                    shared += 1;
                }

                let has_endpoint = first.len() == shared;
                let endpoint = has_endpoint
                    .then_some(*partition[0].1)
                    .unwrap_or(EndpointValue::Absent);
                let mut groups = Vec::new();
                let mut group_start = start + usize::from(has_endpoint);
                while group_start < end {
                    let key = ordered[group_start].0;
                    let edge = key[shared];
                    let mut group_end = group_start + 1;
                    while group_end < end &&
                        ordered[group_end].0.len() > shared &&
                        ordered[group_end].0[shared] == edge
                    {
                        group_end += 1;
                    }
                    groups.push((edge, group_start, group_end));
                    group_start = group_end;
                }

                work.push(RebuildWork::Assemble {
                    prefix: first[depth..shared].to_vec(),
                    endpoint,
                    child_edges: groups.iter().map(|(edge, _, _)| *edge).collect(),
                });
                for (_, child_start, child_end) in groups.into_iter().rev() {
                    work.push(RebuildWork::Enter {
                        start: child_start,
                        end: child_end,
                        depth: shared + 1,
                    });
                }
            }
            RebuildWork::Assemble {
                prefix,
                endpoint,
                child_edges,
            } => {
                let child_count = child_edges.len();
                let child_start = completed
                    .len()
                    .checked_sub(child_count)
                    .expect("every child completes before its parent");
                let child_refs = completed.split_off(child_start);
                let children = child_edges
                    .into_iter()
                    .zip(child_refs)
                    .map(|(edge, target)| DagChild { edge, target })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let node = CanonicalDagNode::new(prefix, endpoint, children)
                    .expect("an oracle partition is canonical");
                completed.push(admit_node(store, &node));
            }
        }
    }

    assert_eq!(completed.len(), 1, "the oracle produces exactly one root");
    completed.pop().expect("the oracle root exists")
}

pub(super) fn admit_node<S>(store: &S, node: &CanonicalDagNode) -> ObjectRefV1
where
    S: MutableObjectStore,
    S::Error: Debug,
{
    let bytes: Arc<[u8]> = encode(node).into();
    let reference = node.object_ref();
    let admission = store
        .put_if_absent_or_equal(reference, bytes)
        .expect("an oracle node is admitted");
    assert!(matches!(admission, ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent));
    reference
}

#[test]
fn rebuild_oracle_is_stack_safe_for_a_hundred_thousand_byte_path() {
    std::thread::Builder::new()
        .name("canonical-rebuild-oracle-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let store = super::support::TracingStore::default();
            let entries = Entries::from([(
                vec![0x5a; 100_000],
                EndpointValue::Multiplicity(std::num::NonZeroU64::new(1).unwrap()),
            )]);
            let root = rebuild(&store, &entries);
            assert!(root.encoded_len() > 100_000);
        })
        .unwrap()
        .join()
        .unwrap();
}
