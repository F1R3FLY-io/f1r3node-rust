//! Executable evidence that the protobuf recursion ceiling has been removed.
//!
//! `Par` is the descriptor-derived feedback vertex of the recursive rhoapi
//! message graph. Its generated `prost::Message` implementation delegates
//! encoding, length measurement, field merging, and clearing to explicit
//! machines. Every remaining Prost-derived message reaches that feedback
//! vertex within the statically bounded residual graph height, so term depth no
//! longer consumes native stack or `DecodeContext` recursion budget.

use models::casper::v1::{
    continuation_at_name_response, rho_data_response, ContinuationAtNamePayload,
    ContinuationAtNameResponse, RhoDataPayload, RhoDataResponse,
};
use models::casper::{
    ContinuationsWithBlockInfo, DataAtNameByBlockQuery, DataWithBlockInfo, WaitingContinuationInfo,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path};
use models::rust::rholang::{protobuf_decoder, protobuf_encoder};
use models::rust::utils::new_gint_par;
use prost::encoding::{encode_key, WireType};
use prost::Message;

const DEPTH: usize = 4_096;
const SMALL_STACK: usize = 256 * 1024;

const ENVELOPES: &[&str] = &[
    "Par",
    "DataAtNameByBlockQuery.par",
    "DataWithBlockInfo.postBlockData[0]",
    "RhoDataPayload.par[0]",
    "WaitingContinuationInfo.postBlockContinuation",
    "RhoDataResponse.payload.par[0]",
    "ContinuationsWithBlockInfo.postBlockContinuation",
    "ContinuationAtNamePayload.postBlockContinuation",
    "ContinuationAtNameResponse.postBlockContinuation",
];

fn elist(par: Par) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![par],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        par = elist(par);
    }
    par
}

fn par_depth(par: &Par) -> usize {
    let mut depth = 0usize;
    let mut cursor = par;
    loop {
        match cursor
            .exprs
            .first()
            .and_then(|expr| expr.expr_instance.as_ref())
        {
            Some(ExprInstance::EListBody(list)) if !list.ps.is_empty() => {
                depth += 1;
                cursor = &list.ps[0];
            }
            _ => return depth,
        }
    }
}

fn assert_fixed_point<M: Message + Default>(value: M, label: &str) {
    let bytes = value.encode_to_vec();
    let decoded = M::decode(bytes.as_slice())
        .unwrap_or_else(|error| panic!("{label}: stack-safe decode failed: {error:?}"));
    assert_eq!(
        decoded.encode_to_vec(),
        bytes,
        "{label}: protobuf round trip changed the bytes"
    );
}

fn waiting(term: Par) -> WaitingContinuationInfo {
    WaitingContinuationInfo {
        post_block_patterns: Vec::new(),
        post_block_continuation: Some(term),
    }
}

fn continuations(term: Par) -> ContinuationsWithBlockInfo {
    ContinuationsWithBlockInfo {
        post_block_continuations: vec![waiting(term)],
        block: None,
    }
}

fn continuation_payload(term: Par) -> ContinuationAtNamePayload {
    ContinuationAtNamePayload {
        block_results: vec![continuations(term)],
        length: 0,
    }
}

fn assert_envelope(index: usize, term: Par) {
    match index {
        0 => assert_fixed_point(term, ENVELOPES[index]),
        1 => assert_fixed_point(
            DataAtNameByBlockQuery {
                par: Some(term),
                block_hash: String::new(),
                use_pre_state_hash: false,
            },
            ENVELOPES[index],
        ),
        2 => assert_fixed_point(
            DataWithBlockInfo {
                post_block_data: vec![term],
                block: None,
            },
            ENVELOPES[index],
        ),
        3 => assert_fixed_point(
            RhoDataPayload {
                par: vec![term],
                block: None,
            },
            ENVELOPES[index],
        ),
        4 => assert_fixed_point(waiting(term), ENVELOPES[index]),
        5 => assert_fixed_point(
            RhoDataResponse {
                message: Some(rho_data_response::Message::Payload(RhoDataPayload {
                    par: vec![term],
                    block: None,
                })),
            },
            ENVELOPES[index],
        ),
        6 => assert_fixed_point(continuations(term), ENVELOPES[index]),
        7 => assert_fixed_point(continuation_payload(term), ENVELOPES[index]),
        8 => assert_fixed_point(
            ContinuationAtNameResponse {
                message: Some(continuation_at_name_response::Message::Payload(
                    continuation_payload(term),
                )),
            },
            ENVELOPES[index],
        ),
        _ => panic!("protobuf stack-safety envelope index {index} is unregistered"),
    }
}

#[test]
fn par_message_round_trip_is_stack_safe_and_unbounded_by_decode_context() {
    std::thread::Builder::new()
        .name("par-message-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let term = nested_list(DEPTH);
            assert_eq!(par_depth(&term), DEPTH);

            let message_bytes = term.encode_to_vec();
            let pda_bytes = protobuf_encoder::encode_to_vec(&term);
            assert_eq!(message_bytes, pda_bytes);
            assert_eq!(term.encoded_len(), message_bytes.len());
            assert_eq!(protobuf_encoder::encoded_len(&term), message_bytes.len());

            let message = Par::decode(message_bytes.as_slice())
                .expect("manual Message decoder accepts depth 4096");
            let generated = protobuf_decoder::decode_par(message_bytes.as_slice())
                .expect("generated protobuf machine accepts depth 4096");
            assert_eq!(par_depth(&message), DEPTH);
            assert_eq!(par_depth(&generated), DEPTH);
            assert_eq!(message.encode_to_vec(), message_bytes);
            assert_eq!(generated.encode_to_vec(), message_bytes);
        })
        .expect("spawn the fixed small-stack Par Message probe")
        .join()
        .expect("protobuf Message operations must be independent of native stack depth");
}

#[test]
fn every_production_envelope_crosses_the_par_feedback_vertex_stack_safely() {
    std::thread::Builder::new()
        .name("par-envelope-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            assert!(!ENVELOPES.is_empty());
            for index in 0..ENVELOPES.len() {
                let term = nested_list(DEPTH);
                assert_eq!(par_depth(&term), DEPTH, "{} fixture", ENVELOPES[index]);
                assert_envelope(index, term);
            }
        })
        .expect("spawn the fixed small-stack envelope probe")
        .join()
        .expect("production protobuf envelopes must not restore recursive traversal");
}

#[test]
fn message_clear_tears_down_a_deep_term_iteratively() {
    std::thread::Builder::new()
        .name("par-clear-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let mut term = nested_list(DEPTH);
            Message::clear(&mut term);
            assert_eq!(term, Par::default());
            assert!(term.encode_to_vec().is_empty());
        })
        .expect("spawn the fixed small-stack clear probe")
        .join()
        .expect("Message::clear must use iterative teardown");
}

#[test]
fn message_clear_matches_protobuf_default_semantics() {
    let mut term = nested_list(64);
    term.locally_free = vec![0xA5, 0x5A];
    term.connective_used = true;

    Message::clear(&mut term);

    assert!(term.sends.is_empty());
    assert!(term.receives.is_empty());
    assert!(term.news.is_empty());
    assert!(term.exprs.is_empty());
    assert!(term.matches.is_empty());
    assert!(term.unforgeables.is_empty());
    assert!(term.bundles.is_empty());
    assert!(term.connectives.is_empty());
    assert!(term.conditionals.is_empty());
    assert!(term.locally_free.is_empty());
    assert!(!term.connective_used);
    assert_eq!(term.encode_to_vec(), Par::default().encode_to_vec());
}

#[test]
fn canonical_path_and_protobuf_accept_the_same_deep_term() {
    std::thread::Builder::new()
        .name("par-path-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let term = nested_list(DEPTH);
            let path = encode_trie_path(&term);
            let from_path = decode_trie_path(&path).expect("canonical path accepts depth 4096");
            assert_eq!(encode_trie_path(&from_path), path);

            let protobuf = term.encode_to_vec();
            let from_protobuf = Par::decode(protobuf.as_slice())
                .expect("protobuf accepts the same depth-4096 term");
            assert_eq!(encode_trie_path(&from_protobuf), path);
        })
        .expect("spawn the fixed small-stack path/protobuf probe")
        .join()
        .expect("neither canonical path nor protobuf may impose a depth ceiling");
}

#[test]
fn message_unknown_group_skip_is_iterative() {
    const GROUP_DEPTH: usize = 16_384;
    std::thread::Builder::new()
        .name("par-message-groups-depth-16384".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let mut bytes = Vec::with_capacity(GROUP_DEPTH * 2);
            for _ in 0..GROUP_DEPTH {
                encode_key(15, WireType::StartGroup, &mut bytes);
            }
            for _ in 0..GROUP_DEPTH {
                encode_key(15, WireType::EndGroup, &mut bytes);
            }
            assert_eq!(Par::decode(bytes.as_slice()).unwrap(), Par::default());
            assert_eq!(
                protobuf_decoder::decode_par(bytes.as_slice()).unwrap(),
                Par::default()
            );
        })
        .expect("spawn the fixed small-stack unknown-group probe")
        .join()
        .expect("unknown protobuf groups must use an explicit tag stack");
}
