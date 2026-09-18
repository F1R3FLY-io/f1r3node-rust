use proptest::prelude::*;

use super::{checked_add, delimited_len, finish_node, EmitMode, Node, Piece, ProtobufRegionError};

#[path = "../par_corpus/mod.rs"]
mod par_corpus;

use std::cell::RefCell;

use super::{Builder, ProtobufCutPlan, ProtobufCutStats, ProtobufRegion};
use crate::rust::rholang::match_schema::RhoMatchProgram;
use crate::rust::rholang::match_schema_tables::PAR_PROTOBUF_REGION_PROGRAM;
use crate::rust::rholang::protobuf_encoder;

struct PublishedNode {
    source: ProtobufRegion,
    output: [prost::bytes::Bytes; 3],
}

fn verify_epathmap_value_frame_partition(
    root: &ProtobufRegion,
    plan: &ProtobufCutPlan,
    observations: &[PublishedNode],
) -> usize {
    use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalWireType};
    let mut frame_count = 0;
    for group in &plan.epathmaps {
        let start = group.prefix.absolute_range().start;
        let mut end = group.prefix.absolute_range().end;
        let mut captured_end = None;
        for observation in observations {
            let base = observation.source.absolute_range().start;
            let mut fields = CanonicalFieldCursor::in_prost_slot_order(observation.source.as_bytes());
            while let Some(field) = fields.next_field().unwrap() {
                if field.wire_type == CanonicalWireType::LengthDelimited
                    && base + field.payload_range.start == start
                {
                    assert!(captured_end.replace(base + field.payload_range.end).is_none());
                }
            }
        }
        let mut rebuilt = group.prefix.as_bytes().to_vec();
        for cut in &plan.cuts[group.cuts.clone()] {
            frame_count += 1;
            let body = cut.region.as_bytes();
            let mut header = Vec::new();
            prost::encoding::encode_varint(body.len() as u64, &mut header);
            assert_eq!(end + header.len(), cut.region.absolute_range().start);
            assert_eq!(header, &root.as_bytes()[end..end + header.len()]);
            rebuilt.extend_from_slice(&header);
            rebuilt.extend_from_slice(body);
            end = cut.region.absolute_range().end;
        }
        assert_eq!(Some(end), captured_end);
        assert_eq!(rebuilt, &root.as_bytes()[start..end]);
    }
    frame_count
}

thread_local! {
    static PUBLISHED: RefCell<Option<Vec<PublishedNode>>> = const { RefCell::new(None) };
}

pub(super) fn record_publication(index: usize, source: &ProtobufRegion, builder: &Builder) {
    PUBLISHED.with(|capture| {
        let mut capture = capture.borrow_mut();
        let Some(observations) = capture.as_mut() else {
            return;
        };
        assert_eq!(index, observations.len());
        let plan = ProtobufCutPlan {
            nodes: builder.nodes.clone(),
            root: index,
            paths: Vec::new(),
            cuts: builder.cuts.clone(),
            epathmaps: builder.epathmaps.clone(),
            stats: ProtobufCutStats::default(),
        };
        observations.push(PublishedNode {
            source: source.clone(),
            output: [
                plan.reconstructed_bytes(),
                plan.shallow_bytes(),
                plan.compact_shell_bytes(),
            ],
        });
    });
}

struct Capture;
impl Drop for Capture {
    fn drop(&mut self) {
        PUBLISHED.with(|capture| {
            capture.borrow_mut().take();
        });
    }
}

fn capture_build(
    root: ProtobufRegion,
    schema: &'static RhoMatchProgram,
) -> (
    Result<ProtobufCutPlan, ProtobufRegionError>,
    Vec<PublishedNode>,
) {
    PUBLISHED.with(|capture| {
        assert!(capture.borrow().is_none());
        *capture.borrow_mut() = Some(Vec::new());
    });
    let guard = Capture;
    let result = super::cut_at_par(root, schema);
    let observations = PUBLISHED.with(|capture| capture.borrow_mut().take().unwrap());
    drop(guard);
    (result, observations)
}

fn verify_publication(bytes: Vec<u8>) -> usize {
    let root = ProtobufRegion::from(bytes);
    let (result, observations) = capture_build(root.clone(), &PAR_PROTOBUF_REGION_PROGRAM);
    let plan = result.unwrap();
    assert_eq!(plan.root + 1, plan.nodes.len());
    assert_eq!(observations.len(), plan.nodes.len());
    for (index, path) in plan.paths.iter().enumerate() {
        if let Some(parent) = path.parent {
            assert!(parent < index);
        }
    }
    for cut in &plan.cuts {
        assert!(cut.path_leaf < plan.paths.len());
        let range = cut.region.absolute_range();
        assert_eq!(cut.region.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            cut.region.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
    }
    for group in &plan.epathmaps {
        assert!(group.path_leaf < plan.paths.len());
        let range = group.prefix.absolute_range();
        assert_eq!(group.prefix.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            group.prefix.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
    }
    let value_frames = verify_epathmap_value_frame_partition(&root, &plan, &observations);
    let mut ranks = Vec::new();
    for (index, (node, observed)) in plan.nodes.iter().zip(&observations).enumerate() {
        let range = observed.source.absolute_range();
        assert_eq!(observed.source.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            observed.source.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
        let mut rank = 1usize;
        for piece in &node.pieces {
            match piece {
                Piece::Child { node, .. } => {
                    assert!(*node < index);
                    rank += 1 + ranks[*node];
                }
                Piece::Par { cut, .. } => {
                    assert!(*cut < plan.cuts.len());
                    rank += 1;
                }
                Piece::EPathMap { group, .. } => {
                    let group = &plan.epathmaps[*group];
                    assert!(group.cuts.start <= group.cuts.end);
                    assert!(group.cuts.end <= plan.cuts.len());
                    rank += 1;
                }
                Piece::Borrowed(region) => {
                    let range = region.absolute_range();
                    assert_eq!(region.as_bytes(), &root.as_bytes()[range.clone()]);
                    assert_eq!(
                        region.as_bytes().as_ptr(),
                        root.as_bytes()[range.start..].as_ptr()
                    );
                    rank += 1;
                }
            }
        }
        ranks.push(rank);
        assert!(rank > 0);
        let mut retained = plan.clone();
        retained.root = index;
        let after = [
            retained.reconstructed_bytes(),
            retained.shallow_bytes(),
            retained.compact_shell_bytes(),
        ];
        assert_eq!(after, observed.output);
        assert_eq!(after[0].as_ref(), observed.source.as_bytes());
        assert_eq!([after[0].len(), after[1].len(), after[2].len()], [
            node.original_len,
            node.shallow_len,
            node.compact_len
        ]);
        retained.nodes.truncate(index + 1);
        assert_eq!(
            [
                retained.reconstructed_bytes(),
                retained.shallow_bytes(),
                retained.compact_shell_bytes()
            ],
            after
        );
    }
    assert_eq!(plan.reconstructed_bytes().as_ref(), root.as_bytes());
    value_frames
}

#[test]
fn epathmap_publication_frames_include_empty_and_distinct_map_values() {
    use crate::rhoapi::{Expr, Par};
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::rhoapi_ext::EPathMap;
    let map = EPathMap::new_map(
        [
            (par_corpus::gint(1), Par::default()),
            (par_corpus::gint(2), par_corpus::gint(7)),
            (par_corpus::gint(3), par_corpus::gint(9)),
        ],
        Vec::new(), false, None,
    );
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }]);
    assert_eq!(verify_publication(protobuf_encoder::encode_to_vec(&par)), 3);
}

#[test]
fn every_published_node_preserves_source_caches_indices_and_prior_meaning() {
    let corpus = par_corpus::par_corpus();
    assert!(corpus.len() >= 40);
    for (_, par) in corpus {
        verify_publication(protobuf_encoder::encode_to_vec(&par));
    }
    verify_publication(protobuf_encoder::encode_to_vec(
        &par_corpus::all_par_fields(),
    ));
    let mut unknown = protobuf_encoder::encode_to_vec(&par_corpus::all_par_fields());
    unknown.extend_from_slice(&[0xfa, 0x07, 0x02, 7, 9]);
    verify_publication(unknown);
}

proptest! {
    #[test]
    fn generated_publications_preserve_actual_frame_bytes(values in prop::collection::vec(any::<i64>(), 0..24)) {
        use crate::rhoapi::{EMap, Expr, KeyValuePair, Par};
        use crate::rhoapi::expr::ExprInstance;
        let entries = values.into_iter().enumerate().map(|(key, value)| KeyValuePair {
            key: Some(par_corpus::gint(key as i64)), value: Some(par_corpus::gint(value)),
        }).chain([KeyValuePair::default()]).collect();
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap { kvs: entries, ..EMap::default() })),
        }]);
        verify_publication(protobuf_encoder::encode_to_vec(&par));
    }
}

#[test]
fn late_framing_failure_never_publishes_the_root() {
    let root = ProtobufRegion::from(vec![0x0a, 0, 0xff]);
    let (result, observations) = capture_build(root, &PAR_PROTOBUF_REGION_PROGRAM);
    assert!(matches!(result, Err(ProtobufRegionError::Framing(_))));
    assert_eq!(observations.len(), 1);
    assert!(observations[0].source.is_empty());
}

fn widened_delimited_length(tag: u32, body: usize) -> Result<usize, ProtobufRegionError> {
    let key = u64::from(tag) * 8 + 2;
    let varint_len = |value: u64| -> u128 {
        let bits = 64 - value.leading_zeros();
        u128::from(bits.max(1).div_ceil(7))
    };
    u64::try_from(body)
        .ok()
        .and_then(|size| {
            let total = varint_len(key) + varint_len(size) + body as u128;
            usize::try_from(total).ok()
        })
        .ok_or(ProtobufRegionError::SizeOverflow)
}

proptest! {
    #[test]
    fn checked_add_matches_machine_word_arithmetic(left in any::<usize>(), right in any::<usize>()) {
        prop_assert_eq!(checked_add(left, right), left.checked_add(right).ok_or(ProtobufRegionError::SizeOverflow));
    }

    #[test]
    fn delimited_length_matches_widened_arithmetic(tag in 1u32..=536_870_911, body in any::<usize>()) {
        prop_assert_eq!(delimited_len(tag, body), widened_delimited_length(tag, body));
    }
}

#[test]
fn delimited_length_covers_key_and_payload_width_transitions() {
    for tag in [
        1, 15, 16, 2047, 2048, 262143, 262144, 33554431, 33554432, 536870911,
    ] {
        for body in [0, 1, 127, 128, 16383, 16384, usize::MAX - 16, usize::MAX] {
            assert_eq!(
                delimited_len(tag, body),
                widened_delimited_length(tag, body)
            );
        }
    }
}

#[test]
fn overflow_in_each_cached_mode_refuses_finished_node_publication() {
    for mode in [EmitMode::Compact, EmitMode::Shallow, EmitMode::Original] {
        let prior = vec![Node {
            pieces: Vec::new(),
            compact_len: if mode == EmitMode::Compact {
                usize::MAX
            } else {
                0
            },
            shallow_len: if mode == EmitMode::Shallow {
                usize::MAX
            } else {
                0
            },
            original_len: if mode == EmitMode::Original {
                usize::MAX
            } else {
                0
            },
        }];
        let before = (
            prior[0].compact_len,
            prior[0].shallow_len,
            prior[0].original_len,
        );
        let result = finish_node(
            vec![Piece::Child {
                tag: 1,
                node: 0,
                retain_empty_shell: true,
            }],
            &prior,
            &[],
            &[],
        );
        assert!(matches!(result, Err(ProtobufRegionError::SizeOverflow)));
        assert_eq!(prior.len(), 1);
        assert_eq!(
            (
                prior[0].compact_len,
                prior[0].shallow_len,
                prior[0].original_len
            ),
            before
        );
    }
    assert_eq!(
        checked_add(usize::MAX, 1),
        Err(ProtobufRegionError::SizeOverflow)
    );
    assert_eq!(checked_add(usize::MAX, 0), Ok(usize::MAX));
    assert_eq!(
        delimited_len(1, usize::MAX),
        Err(ProtobufRegionError::SizeOverflow)
    );
}
