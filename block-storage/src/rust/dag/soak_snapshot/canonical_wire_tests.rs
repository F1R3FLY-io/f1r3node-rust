use std::fmt::Write;

use models::rust::casper::protocol::casper_message::Justification;

use super::*;

fn bytes(value: &[u8]) -> String {
    format!(
        "[{}]",
        value
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(";")
    )
}

fn list<T>(values: impl IntoIterator<Item = T>, render: impl Fn(T) -> String) -> String {
    format!(
        "[{}]",
        values.into_iter().map(render).collect::<Vec<_>>().join(";")
    )
}

fn option<T>(value: Option<T>, render: impl Fn(T) -> String) -> String {
    value.map_or_else(|| "None".to_owned(), |v| format!("(Some {})", render(v)))
}

fn availability(value: &Availability<BlockHash>) -> String {
    match value {
        Availability::Absent => "None".to_owned(),
        Availability::Present(hash) => option(Some(hash), |h| bytes(h)),
    }
}

fn metadata(value: &BlockMetadata) -> String {
    format!(
        "{{| metadata_block_hash := {}; metadata_parents := {}; metadata_sender := {}; metadata_justifications := {}; metadata_weights := {}; metadata_block_number := {}; metadata_sequence_number := {}; metadata_invalid := {}; metadata_directly_finalized := {}; metadata_finalized := {}; metadata_fault_tolerance_bits := {}; metadata_merge_base := {} |}}",
        bytes(&value.block_hash),
        list(&value.parents, |v| bytes(v)),
        bytes(&value.sender),
        list(&value.justifications, |j| format!("({}, {})", bytes(&j.validator), bytes(&j.latest_block_hash))),
        list(&value.weight_map, |(key, n)| format!("({}, {})", bytes(key), *n as u64)),
        value.block_number as u64,
        value.sequence_number as u32,
        value.invalid,
        value.directly_finalized,
        value.finalized,
        value.fault_tolerance_value.to_bits(),
        bytes(&value.merge_base),
    )
}

fn model(value: &SnapshotData) -> String {
    let mut out = String::from("{| ");
    let mut field = |name: &str, contents: String| {
        writeln!(out, "snapshot_{name} := {contents};").unwrap();
    };
    field("schema_version", value.schema_version.to_string());
    field("scope", bytes(value.scope.as_bytes()));
    let limits = &value.limits;
    field(
        "limits",
        format!(
            "({}, {})",
            list(
                [
                    limits.read.max_value_bytes as u64,
                    limits.read.max_total_bytes as u64,
                    limits.read.max_records as u64,
                    limits.read.max_operations as u64,
                    limits.block_decode.max_compressed_bytes as u64,
                    limits.block_decode.max_decompressed_bytes as u64,
                    limits.block_decode.max_expansion_ratio as u64,
                    limits.max_blocks as u64,
                    limits.max_validators as u64,
                    limits.max_edges as u64,
                    limits.max_work as u64,
                    limits.lock_wait.as_secs(),
                ],
                |n| n.to_string()
            ),
            limits.lock_wait.subsec_nanos()
        ),
    );
    field(
        "usage",
        list(
            [
                value.usage.bytes,
                value.usage.records,
                value.usage.operations,
            ],
            |n| n.to_string(),
        ),
    );
    field(
        "coverage",
        format!(
            "({}, ({}, {}))",
            value.coverage.held_blocks,
            value.coverage.complete_held_dag,
            value.coverage.requested_bodies
        ),
    );
    field("generation", value.insertion_generation.to_string());
    field(
        "transactions",
        list(&value.transactions, |t| {
            format!(
                "({}, ({}, ({}, {})))",
                bytes(t.environment.as_bytes()),
                t.last_txn_id_before_open,
                t.txn_id,
                option(t.last_txn_id_after_validation, |n| n.to_string())
            )
        }),
    );
    field("dag_set", list(&value.dag_set, |h| bytes(h)));
    field(
        "child_map",
        list(&value.child_map, |(h, children)| {
            format!("({}, {})", bytes(h), list(children, |c| bytes(c)))
        }),
    );
    field(
        "height_map",
        list(&value.height_map, |(n, hashes)| {
            format!("({}, {})", *n as u64, list(hashes, |h| bytes(h)))
        }),
    );
    field(
        "block_number_map",
        list(&value.block_number_map, |(h, n)| {
            format!("({}, {})", bytes(h), *n as u64)
        }),
    );
    for (name, map) in [
        ("main_parent_map", &value.main_parent_map),
        ("self_justification_map", &value.self_justification_map),
    ] {
        field(
            name,
            list(map, |(a, b)| format!("({}, {})", bytes(a), bytes(b))),
        );
    }
    field(
        "last_finalized_block",
        option(value.last_finalized_block.as_ref(), |(h, n)| {
            format!("({}, {})", bytes(h), *n as u64)
        }),
    );
    field(
        "finalized_block_set",
        list(&value.finalized_block_set, |h| bytes(h)),
    );
    field(
        "latest_messages",
        list(&value.latest_messages, |(v, h)| {
            format!("({}, {})", bytes(v), bytes(h))
        }),
    );
    field(
        "invalid_blocks",
        list(&value.invalid_blocks, |(h, m)| {
            format!("({}, {})", bytes(h), metadata(m))
        }),
    );
    field(
        "blocks",
        list(&value.blocks, |(h, b)| {
            format!(
                "({}, ({}, ({}, {})))",
                bytes(h),
                metadata(&b.metadata),
                availability(&b.floor),
                availability(&b.frontier)
            )
        }),
    );
    field(
        "bodies",
        list(&value.bodies, |(h, body)| {
            format!("({}, {})", bytes(h), match body {
                BlockBody::NotHeld => "None".to_owned(),
                BlockBody::Held(b) => option(Some(b), |v| bytes(v)),
            })
        }),
    );
    write!(out, "snapshot_work := {} |}}", value.work).unwrap();
    out
}

fn fixture() -> SnapshotData {
    let hash = |v| BlockHash::from(vec![v]);
    let metadata = BlockMetadata {
        block_hash: hash(1),
        parents: vec![hash(3), hash(2)],
        sender: hash(4),
        justifications: vec![
            Justification {
                validator: hash(9),
                latest_block_hash: hash(2),
            },
            Justification {
                validator: hash(5),
                latest_block_hash: hash(3),
            },
        ],
        weight_map: [(hash(7), i64::MAX), (hash(6), i64::MIN)]
            .into_iter()
            .collect(),
        block_number: -1,
        sequence_number: i32::MIN,
        invalid: true,
        directly_finalized: false,
        finalized: true,
        fault_tolerance_value: f32::from_bits(0x7fc0_1234),
        merge_base: hash(2),
    };
    SnapshotData {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        scope: SNAPSHOT_SCOPE,
        limits: CaptureLimits {
            read: ReadLimits {
                max_value_bytes: 65536,
                max_total_bytes: 1_000_000,
                max_records: 10,
                max_operations: 20,
            },
            block_decode: BlockDecodeLimits {
                max_compressed_bytes: 500,
                max_decompressed_bytes: 600,
                max_expansion_ratio: 7,
            },
            max_blocks: 100,
            max_validators: 8,
            max_edges: 200,
            max_work: 10_000_000,
            lock_wait: Duration::new(2, 999_999_999),
        },
        coverage: Coverage {
            held_blocks: 2,
            complete_held_dag: true,
            requested_bodies: 3,
        },
        insertion_generation: u64::MAX,
        transactions: vec![
            TransactionIdentity {
                environment: "a\0é".to_owned(),
                last_txn_id_before_open: 1,
                txn_id: usize::MAX,
                last_txn_id_after_validation: Some(0),
            },
            TransactionIdentity {
                environment: "b".to_owned(),
                last_txn_id_before_open: 2,
                txn_id: 3,
                last_txn_id_after_validation: None,
            },
        ],
        usage: ReadUsage {
            bytes: 123,
            records: 4,
            operations: 5,
        },
        work: 0,
        dag_set: [hash(1), hash(2)].into_iter().collect(),
        child_map: [
            (hash(2), BTreeSet::new()),
            (hash(1), [hash(3), hash(2)].into_iter().collect()),
        ]
        .into_iter()
        .collect(),
        height_map: [
            (i64::MAX, [hash(2)].into_iter().collect()),
            (i64::MIN, [hash(1)].into_iter().collect()),
        ]
        .into_iter()
        .collect(),
        block_number_map: [(hash(2), i64::MIN), (hash(1), i64::MAX)]
            .into_iter()
            .collect(),
        main_parent_map: [(hash(1), hash(2)), (hash(2), hash(3))]
            .into_iter()
            .collect(),
        self_justification_map: [(hash(1), hash(3)), (hash(2), hash(2))]
            .into_iter()
            .collect(),
        last_finalized_block: Some((hash(1), i64::MIN)),
        finalized_block_set: [hash(2), hash(1)].into_iter().collect(),
        latest_messages: [(hash(8), hash(1)), (hash(7), hash(2))]
            .into_iter()
            .collect(),
        invalid_blocks: [(hash(1), metadata.clone())].into_iter().collect(),
        blocks: [(hash(1), DetachedBlock {
            metadata,
            floor: Availability::Absent,
            frontier: Availability::Present(hash(2)),
        })]
        .into_iter()
        .collect(),
        bodies: [
            (hash(1), BlockBody::Held(vec![0, 255, 128, 1])),
            (hash(2), BlockBody::Held(vec![])),
            (hash(3), BlockBody::NotHeld),
        ]
        .into_iter()
        .collect(),
    }
}

fn cases() -> Vec<(String, SnapshotData, usize)> {
    let base = fixture();
    let mut cases = vec![("base".to_owned(), base.clone(), 0)];
    macro_rules! case {
        ($name:expr, $edit:expr) => {{
            let mut value = base.clone();
            ($edit)(&mut value);
            cases.push(($name.to_owned(), value, 0));
        }};
    }
    case!("schema", |v: &mut SnapshotData| v.schema_version = u32::MAX);
    case!("scope", |v: &mut SnapshotData| v.scope = "");
    macro_rules! limit {
        ($($member:ident).+) => {
            case!(stringify!($($member).+), |v: &mut SnapshotData| v.limits.$($member).+ += 1);
        };
    }
    limit!(read.max_value_bytes);
    limit!(read.max_total_bytes);
    limit!(read.max_records);
    limit!(read.max_operations);
    limit!(block_decode.max_compressed_bytes);
    limit!(block_decode.max_decompressed_bytes);
    limit!(block_decode.max_expansion_ratio);
    limit!(max_blocks);
    limit!(max_validators);
    limit!(max_edges);
    limit!(max_work);
    case!("duration_seconds", |v: &mut SnapshotData| v
        .limits
        .lock_wait =
        Duration::new(u64::MAX, 999_999_999));
    case!("duration_nanos", |v: &mut SnapshotData| v
        .limits
        .lock_wait =
        Duration::new(2, 0));
    case!("usage_bytes", |v: &mut SnapshotData| v.usage.bytes += 1);
    case!("usage_records", |v: &mut SnapshotData| v.usage.records += 1);
    case!("usage_operations", |v: &mut SnapshotData| v
        .usage
        .operations +=
        1);
    case!("coverage_held", |v: &mut SnapshotData| v
        .coverage
        .held_blocks +=
        1);
    case!("coverage_complete", |v: &mut SnapshotData| v
        .coverage
        .complete_held_dag =
        false);
    case!("coverage_requested", |v: &mut SnapshotData| v
        .coverage
        .requested_bodies +=
        1);
    case!("generation", |v: &mut SnapshotData| v
        .insertion_generation =
        0);
    case!("transaction_environment", |v: &mut SnapshotData| v
        .transactions[0]
        .environment =
        "a".to_owned());
    case!("transaction_before", |v: &mut SnapshotData| v
        .transactions[0]
        .last_txn_id_before_open =
        0);
    case!("transaction_open", |v: &mut SnapshotData| v.transactions
        [0]
    .txn_id = 0);
    case!("transaction_after", |v: &mut SnapshotData| v.transactions
        [0]
    .last_txn_id_after_validation =
        None);
    case!("transaction_order", |v: &mut SnapshotData| v
        .transactions
        .reverse());
    macro_rules! empty {
        ($member:ident) => {
            case!(stringify!($member), |v: &mut SnapshotData| v
                .$member
                .clear());
        };
    }
    empty!(transactions);
    empty!(dag_set);
    empty!(child_map);
    empty!(height_map);
    empty!(block_number_map);
    empty!(main_parent_map);
    empty!(self_justification_map);
    empty!(finalized_block_set);
    empty!(latest_messages);
    empty!(invalid_blocks);
    empty!(blocks);
    empty!(bodies);
    case!("last_finalized_none", |v: &mut SnapshotData| v
        .last_finalized_block =
        None);
    case!("last_finalized_height", |v: &mut SnapshotData| v
        .last_finalized_block
        .as_mut()
        .unwrap()
        .1 = 0);
    case!("floor_present_empty", |v: &mut SnapshotData| v
        .blocks
        .values_mut()
        .next()
        .unwrap()
        .floor =
        Availability::Present(vec![].into()));
    case!("frontier_absent", |v: &mut SnapshotData| v
        .blocks
        .values_mut()
        .next()
        .unwrap()
        .frontier =
        Availability::Absent);
    case!("body_not_held", |v: &mut SnapshotData| {
        v.bodies.insert(vec![2].into(), BlockBody::NotHeld);
    });
    case!("body_empty", |v: &mut SnapshotData| {
        v.bodies.insert(vec![1].into(), BlockBody::Held(vec![]));
    });
    macro_rules! meta {
        ($name:expr, $edit:expr) => {
            for invalid in [false, true] {
                case!(&format!("{}_{}", $name, invalid), |v: &mut SnapshotData| {
                    let m = if invalid {
                        v.invalid_blocks.values_mut().next().unwrap()
                    } else {
                        &mut v.blocks.values_mut().next().unwrap().metadata
                    };
                    ($edit)(m);
                });
            }
        };
    }
    meta!("block_hash", |m: &mut BlockMetadata| m.block_hash =
        vec![].into());
    meta!("parents_order", |m: &mut BlockMetadata| m.parents.reverse());
    meta!("parents_count", |m: &mut BlockMetadata| m.parents.clear());
    meta!("sender", |m: &mut BlockMetadata| m.sender = vec![].into());
    meta!("justifications_order", |m: &mut BlockMetadata| m
        .justifications
        .reverse());
    meta!("justifications_count", |m: &mut BlockMetadata| m
        .justifications
        .clear());
    meta!("weights", |m: &mut BlockMetadata| m.weight_map.clear());
    meta!("block_number", |m: &mut BlockMetadata| m.block_number =
        i64::MIN);
    meta!("sequence_number", |m: &mut BlockMetadata| m
        .sequence_number =
        i32::MAX);
    meta!("invalid", |m: &mut BlockMetadata| m.invalid = false);
    meta!("directly_finalized", |m: &mut BlockMetadata| m
        .directly_finalized =
        true);
    meta!("finalized", |m: &mut BlockMetadata| m.finalized = false);
    meta!("float_bits", |m: &mut BlockMetadata| m
        .fault_tolerance_value =
        -0.0);
    meta!("merge_base", |m: &mut BlockMetadata| m.merge_base =
        vec![].into());
    cases.push(("work".to_owned(), base, 17));
    cases
}

#[test]
fn canonical_wire_field_coverage() {
    let directory = std::env::var_os("B11_ROCQ_CASES_DIR").map(std::path::PathBuf::from);
    if let Some(path) = &directory {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut baseline = None;
    let mut index = String::new();
    for (number, (name, data, used)) in cases().into_iter().enumerate() {
        let limit = data.limits.max_work;
        let snapshot = DetachedDagSnapshot::seal(data, WorkMeter { used, limit }).unwrap();
        let wire = snapshot.canonical_bytes();
        if let Some(before) = &baseline {
            assert_ne!(wire, before, "field omission: {name}");
        } else {
            baseline = Some(wire.to_vec());
        }
        assert_eq!(
            snapshot.digest().as_slice(),
            Sha256Hasher::hash(wire.to_vec())
        );
        if let Some(path) = &directory {
            let mut source = format!(
                "From Stdlib Require Import List NArith.\nFrom NodeObservationB11 Require Import Wire Schema MainTheorem.\nImport ListNotations.\nOpen Scope N_scope.\nDefinition input : snapshot := {}.\nDefinition rust_output : wire := {}.\nExample input_valid : valid snapshot_codec input.\nProof. repeat constructor. Qed.\nExample production_correspondence : emit snapshot_codec input = rust_output.\nProof. vm_compute. reflexivity. Qed.\nExample production_roundtrip : parse snapshot_codec rust_output = Some (input, []).\nProof. rewrite <- production_correspondence. replace (emit snapshot_codec input) with (emit snapshot_codec input ++ []) by apply app_nil_r. apply canonical_snapshot_roundtrip. exact input_valid. Qed.\n",
                model(&snapshot), bytes(wire)
            );
            if number == 0 {
                let mut endian = wire.to_vec();
                endian[14..18].reverse();
                let mut tag = wire.to_vec();
                tag[8] ^= 1;
                let mut omitted = wire.to_vec();
                omitted.drain(14..18);
                let mut trailing = wire.to_vec();
                trailing.push(0);
                let mut flag = wire.to_vec();
                let coverage = wire.windows(8).position(|w| w == b"coverage").unwrap();
                flag[coverage + 16] = 0;
                for (name, mutant) in [
                    ("endianness", endian),
                    ("tag", tag),
                    ("omitted_field", omitted),
                    ("truncated_work", wire[..wire.len() - 8].to_vec()),
                    ("trailing_byte", trailing),
                    ("coverage_flag", flag),
                ] {
                    assert_ne!(wire, mutant, "ineffective control: {name}");
                    writeln!(source,
                        "Example reject_{name} : emit snapshot_codec input <> {}.\nProof. vm_compute. discriminate. Qed.",
                        bytes(&mutant)).unwrap();
                }
            }
            let filename = format!("Case{number:03}.v");
            std::fs::write(path.join(&filename), source).unwrap();
            writeln!(
                index,
                "{filename}\t{name}\t{}",
                hex::encode(snapshot.digest())
            )
            .unwrap();
        }
    }
    if let Some(path) = &directory {
        std::fs::write(path.join("cases.tsv"), index).unwrap();
    }
}

#[test]
fn canonical_collections_ignore_insertion_order() {
    let original = fixture();
    let mut reordered = original.clone();
    macro_rules! reverse {
        ($member:ident) => {
            reordered.$member = original.$member.clone().into_iter().rev().collect();
        };
    }
    reverse!(dag_set);
    reverse!(child_map);
    reverse!(height_map);
    reverse!(block_number_map);
    reverse!(main_parent_map);
    reverse!(self_justification_map);
    reverse!(finalized_block_set);
    reverse!(latest_messages);
    reverse!(invalid_blocks);
    reverse!(blocks);
    reverse!(bodies);
    for children in reordered.child_map.values_mut() {
        *children = children.clone().into_iter().rev().collect();
    }
    for hashes in reordered.height_map.values_mut() {
        *hashes = hashes.clone().into_iter().rev().collect();
    }
    for metadata in reordered.invalid_blocks.values_mut() {
        metadata.weight_map = metadata.weight_map.clone().into_iter().rev().collect();
    }
    for block in reordered.blocks.values_mut() {
        block.metadata.weight_map = block
            .metadata
            .weight_map
            .clone()
            .into_iter()
            .rev()
            .collect();
    }
    let encode = |data: SnapshotData| {
        let limit = data.limits.max_work;
        DetachedDagSnapshot::seal(data, WorkMeter { used: 0, limit }).unwrap()
    };
    assert_eq!(
        encode(original).canonical_bytes(),
        encode(reordered).canonical_bytes()
    );
}
