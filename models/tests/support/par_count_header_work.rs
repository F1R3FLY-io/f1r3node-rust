use std::cell::RefCell;
use std::collections::BTreeMap;

use prost::encoding::{encode_key, encode_varint, WireType};

use super::{CanonicalByteRange, CanonicalParRegionIndex, ProtobufRegion};
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::{Connective, ConnectiveBody, EVar, Expr, Par, Var};
use crate::rust::par_bag::{
    canonical_connective_parts, canonical_count_expr_is_hole, CanonicalConnectiveParts,
};
use crate::rust::rholang::match_schema_tables::PAR_PROTOBUF_REGION_PROGRAM;
use crate::rust::rholang::protobuf_encoder;
use crate::rust::rholang::protobuf_schema::ProtobufKind;

#[derive(Default)]
pub(crate) struct Reads {
    positions: BTreeMap<usize, usize>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Reads>> = const { RefCell::new(None) };
}

pub(crate) fn record_byte(address: usize) {
    ACTIVE.with(|active| {
        if let Some(reads) = active.borrow_mut().as_mut() {
            *reads.positions.entry(address).or_default() += 1;
        }
    });
}

pub(crate) fn measure<T>(action: impl FnOnce() -> T) -> (T, Reads) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { ACTIVE.with(|active| *active.borrow_mut() = None); }
    }
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        assert!(active.is_none(), "header work scopes must not overlap");
        *active = Some(Reads::default());
    });
    let _reset = Reset;
    let result = action();
    let reads = ACTIVE.with(|active| active.borrow_mut().take().unwrap());
    (result, reads)
}

impl Reads {
    pub(crate) fn total(&self) -> usize { self.positions.values().sum() }

    pub(crate) fn maximum(&self) -> usize { self.positions.values().copied().max().unwrap_or(0) }

    fn within(&self, root: &ProtobufRegion, limit: usize) -> bool {
        let start = root.as_bytes().as_ptr() as usize;
        let end = start + root.len();
        self.positions
            .iter()
            .all(|(&address, &reads)| start <= address && address < end && reads <= limit)
    }

    fn assert_exact(&self, root: &ProtobufRegion, expected: &BTreeMap<usize, usize>) {
        let start = root.as_bytes().as_ptr() as usize;
        let absolute: BTreeMap<_, _> = expected
            .iter()
            .map(|(&offset, &count)| (start + offset, count))
            .collect();
        assert_eq!(self.positions, absolute);
    }
}

fn append_delimited(
    output: &mut Vec<u8>,
    tag: u32,
    payload: &[u8],
    passes: usize,
    expected: &mut BTreeMap<usize, usize>,
) {
    let start = output.len();
    encode_key(tag, WireType::LengthDelimited, output);
    encode_varint(payload.len() as u64, output);
    expected.extend((start..output.len()).map(|offset| (offset, passes)));
    output.extend_from_slice(payload);
}

fn family_runs(index: CanonicalParRegionIndex) -> [Option<super::CanonicalRegionDelimitedRun>; 10] {
    [
        index.sends,
        index.receives,
        index.news,
        index.exprs,
        index.matches,
        index.unforgeables,
        index.connectives,
        index.bundles,
        index.conditionals,
        index.cost_token_stacks,
    ]
}

#[test]
fn par_count_header_index_and_runs_read_each_position_at_most_twice() {
    for width in [0usize, 1, 2, 129] {
        let mut bytes = Vec::new();
        let mut expected = BTreeMap::new();
        let mut families = 0usize;
        for field in PAR_PROTOBUF_REGION_PROGRAM.protobuf_fields {
            match field.name {
                "locally_free" => {
                    append_delimited(&mut bytes, field.tag, &[1; 128], 1, &mut expected)
                }
                "connective_used" => {
                    let start = bytes.len();
                    encode_key(field.tag, WireType::Varint, &mut bytes);
                    encode_varint(1, &mut bytes);
                    expected.extend((start..bytes.len()).map(|offset| (offset, 1)));
                }
                _ => {
                    assert!(matches!(field.kind, ProtobufKind::RepeatedMessage));
                    families += 1;
                    for _ in 0..width {
                        append_delimited(&mut bytes, field.tag, &[], 2, &mut expected);
                    }
                }
            }
        }
        assert_eq!(families, 10);
        let root = ProtobufRegion::from(bytes);
        let retained = root.clone();
        let (counts, reads) = measure(|| {
            let index = CanonicalParRegionIndex::parse(root.clone()).unwrap();
            assert!(index.connective_used);
            assert_eq!(index.locally_free.as_ref().unwrap().as_bytes(), &[1; 128]);
            family_runs(index).map(|run| {
                let mut count = 0;
                if let Some(run) = run {
                    let mut cursor = run.cursor();
                    while let Some(child) = cursor.next_region().unwrap() {
                        assert!(child.is_empty());
                        count += 1;
                    }
                }
                count
            })
        });
        assert_eq!(counts, [width; 10]);
        reads.assert_exact(&root, &expected);
        assert!(reads.within(&root, 2));
        assert_eq!(reads.total(), 4 * families * width + 5);
        assert_eq!(root.as_bytes().as_ptr(), retained.as_bytes().as_ptr());
        eprintln!(
            "PCBN headers index width={width} reads={} positions={} maximum={}",
            reads.total(),
            reads.positions.len(),
            reads.maximum()
        );
    }
}

#[test]
fn par_count_header_and_or_cursors_share_backing_and_read_two_passes() {
    let large = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString("x".repeat(128))),
    }]);
    for width in [0usize, 1, 4, 129] {
        let children = (0..width)
            .map(|index| {
                if index % 2 == 0 {
                    Par::default()
                } else {
                    large.clone()
                }
            })
            .collect::<Vec<_>>();
        let child_bytes = children
            .iter()
            .map(protobuf_encoder::encode_to_vec)
            .collect::<Vec<_>>();
        let mut body = Vec::new();
        let mut body_expected = BTreeMap::new();
        for child in &child_bytes {
            append_delimited(&mut body, 1, child, 2, &mut body_expected);
        }
        assert_eq!(
            body,
            protobuf_encoder::encode_to_vec(&ConnectiveBody {
                ps: children.clone()
            })
        );
        for (tag, instance) in [
            (
                1,
                ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: children.clone(),
                }),
            ),
            (
                2,
                ConnectiveInstance::ConnOrBody(ConnectiveBody {
                    ps: children.clone(),
                }),
            ),
        ] {
            let mut bytes = Vec::new();
            let mut expected = BTreeMap::new();
            append_delimited(&mut bytes, tag, &body, 1, &mut expected);
            let body_start = bytes.len() - body.len();
            expected.extend(
                body_expected
                    .iter()
                    .map(|(&offset, &reads)| (body_start + offset, reads)),
            );
            assert_eq!(
                bytes,
                protobuf_encoder::encode_to_vec(&Connective {
                    connective_instance: Some(instance)
                })
            );
            let root = ProtobufRegion::from(bytes);
            let (regions, reads) = measure(|| {
                let children = match canonical_connective_parts(root.as_bytes()).unwrap() {
                    CanonicalConnectiveParts::And(children)
                    | CanonicalConnectiveParts::Or(children) => children,
                    other => panic!("expected branch children: {other:?}"),
                };
                let owner = root
                    .subregion(CanonicalByteRange {
                        start: body_start,
                        end: root.len(),
                    })
                    .unwrap();
                let mut cursor = children.bind_owned(owner).unwrap();
                let mut regions = Vec::new();
                while let Some(range) = cursor.next() {
                    regions.push(
                        cursor
                            .body()
                            .subregion(CanonicalByteRange {
                                start: range.start,
                                end: range.end,
                            })
                            .unwrap(),
                    );
                }
                regions
            });
            reads.assert_exact(&root, &expected);
            assert!(reads.within(&root, 2));
            assert_eq!(regions.len(), width);
            for region in &regions {
                assert_eq!(
                    region.as_bytes().as_ptr(),
                    root.as_bytes()[region.absolute_range().start..].as_ptr()
                );
            }
            drop(root);
            for (region, expected_child) in regions.iter().zip(&child_bytes) {
                assert_eq!(region.as_bytes(), expected_child);
            }
            eprintln!(
                "PCBN headers branch tag={tag} width={width} reads={} positions={} maximum={}",
                reads.total(),
                reads.positions.len(),
                reads.maximum()
            );
        }
    }
}

#[test]
fn par_count_header_hole_classifier_has_exact_three_read_wildcard_bound() {
    let mut cases = vec![
        (Expr::default(), false, None),
        (
            Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar { v: None })),
            },
            false,
            None,
        ),
    ];
    let mut variables = vec![
        (Var::default(), false, 1),
        (
            Var {
                var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
            },
            true,
            3,
        ),
    ];
    for level in [i32::MIN, -1, 0, 1, i32::MAX] {
        variables.push((
            Var {
                var_instance: Some(VarInstance::FreeVar(level)),
            },
            true,
            2,
        ));
        variables.push((
            Var {
                var_instance: Some(VarInstance::BoundVar(level)),
            },
            false,
            1,
        ));
    }
    for (variable, hole, key_reads) in variables {
        let variable_len = protobuf_encoder::encode_to_vec(&variable).len();
        cases.push((
            Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar { v: Some(variable) })),
            },
            hole,
            (variable_len != 0).then_some((variable_len, key_reads)),
        ));
    }
    for (index, (expr, expected_hole, repeated_key)) in cases.into_iter().enumerate() {
        let root = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&expr));
        let mut expected = (0..root.len())
            .map(|offset| (offset, 1))
            .collect::<BTreeMap<_, _>>();
        if let Some((variable_len, key_reads)) = repeated_key {
            expected.insert(root.len() - variable_len, key_reads);
        }
        let (classification, reads) = measure(|| canonical_count_expr_is_hole(root.as_bytes()));
        assert_eq!(classification, Some(expected_hole));
        reads.assert_exact(&root, &expected);
        assert!(reads.within(&root, 3));
        if repeated_key.is_some_and(|(_, key_reads)| key_reads == 3) {
            assert_eq!(reads.maximum(), 3);
            assert!(!reads.within(&root, 2));
        }
        eprintln!(
            "PCBN headers hole case={index} reads={} positions={} maximum={}",
            reads.total(),
            reads.positions.len(),
            reads.maximum()
        );
    }
}

#[test]
fn par_count_header_restarted_prefix_control_exceeds_the_position_bound() {
    let width = 32usize;
    let tag = PAR_PROTOBUF_REGION_PROGRAM
        .protobuf_fields
        .iter()
        .find(|field| field.name == "exprs")
        .unwrap()
        .tag;
    let mut bytes = Vec::new();
    let mut expected = BTreeMap::new();
    for _ in 0..width {
        append_delimited(&mut bytes, tag, &[], 2, &mut expected);
    }
    let root = ProtobufRegion::from(bytes);
    let (_, forward) = measure(|| {
        let index = CanonicalParRegionIndex::parse(root.clone()).unwrap();
        let mut cursor = index.exprs.unwrap().cursor();
        let mut count = 0;
        while cursor.next_region().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, width);
    });
    forward.assert_exact(&root, &expected);
    assert!(forward.within(&root, 2));
    let (_, restarted) = measure(|| {
        let index = CanonicalParRegionIndex::parse(root.clone()).unwrap();
        let run = index.exprs.unwrap();
        for position in 0..width {
            let mut cursor = run.cursor();
            for _ in 0..=position {
                assert!(cursor.next_region().unwrap().is_some());
            }
        }
    });
    assert_eq!(forward.total(), 4 * width);
    assert_eq!(restarted.total(), 2 * width + width * (width + 1));
    assert_eq!(restarted.maximum(), width + 1);
    assert!(!restarted.within(&root, 3));
    eprintln!(
        "PCBN headers restart forward={} restarted={} maximum={}",
        forward.total(),
        restarted.total(),
        restarted.maximum()
    );
}
