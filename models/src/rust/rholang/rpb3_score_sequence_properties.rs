//! Score transcript preservation across production RPB3 driver steps.
//!
//! Intended as a test-only child of `protobuf_admission.rs`. The expected
//! transcript is rebuilt from each captured append delta and checked against
//! the root-return arena image at every step. A separate score oracle built
//! from the generated Send values checks the meaning of the appended entries.
//! This tests the executable prefix invariant; it is not a Rust-to-Verus proof.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par, Send};
use crate::rust::par_bag::{with_component_key, PAR_BAG_KEY_DOMAIN};
use crate::rust::rholang::canonical_score_bytes::CanonicalScoreSourceEntry;
use crate::rust::rholang::protobuf_encoder;

fn expected_integer_atom(value: i64) -> Vec<u8> {
    let mut score = vec![1, 1];
    score.extend(((value as u64) ^ (1u64 << 63)).to_be_bytes());
    score
}

fn expected_constructor(code: i32, children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut score = vec![2];
    score.extend(expected_integer_atom(i64::from(code)));
    for child in children {
        score.extend(child);
    }
    score.push(0);
    score
}

fn expected_par_gint(value: i16) -> Vec<u8> {
    expected_constructor(999, [
        expected_constructor(2, [expected_integer_atom(i64::from(value))]),
        expected_integer_atom(0),
    ])
}

fn expected_send(channel: i16, data: &[i16], persistent: bool) -> Vec<u8> {
    let children = std::iter::once(expected_integer_atom(i64::from(persistent)))
        .chain(std::iter::once(expected_par_gint(channel)))
        .chain(data.iter().copied().map(expected_par_gint))
        .chain(std::iter::once(expected_integer_atom(0)));
    expected_constructor(Score::SEND, children)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_nested_send_score_transcript_is_an_append_only_driver_sequence(
        channel_value in any::<i16>(),
        data_values in prop::collection::vec(any::<i16>(), 1..4),
        persistent in any::<bool>(),
    ) {
        let par = |value: i16| Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(value))),
        }]);
        let expected_score = expected_send(channel_value, &data_values, persistent);
        let send = Send {
            chan: Some(par(channel_value)),
            data: data_values.iter().copied().map(par).collect(),
            persistent,
            ..Send::default()
        };
        // The legacy sorter is a second route from the generated value to a
        // score key; it does not read admission's score arena or driver trace.
        let sort_score = with_component_key(&send, |key| {
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len = usize::try_from(u64::from_be_bytes(key[trailer..].try_into().unwrap()))
                .unwrap();
            let start = PAR_BAG_KEY_DOMAIN.len() + 1;
            key[start..start + score_len].to_vec()
        });
        prop_assert_eq!(sort_score.as_slice(), expected_score.as_slice());
        let bytes = protobuf_encoder::encode_to_vec(&send);

        rpb3_root_finish_trace::start();
        let result = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &SEND_RHO_MATCH_PROGRAM,
        );
        let trace = rpb3_root_finish_trace::take();
        prop_assert!(result.is_ok(), "generated Send rejected: {:?}", result.as_ref().err());
        let admission = result.unwrap();
        let final_entries = trace.score_arena.as_ref().expect("root return records score image");

        let mut prefix = Vec::<CanonicalScoreSourceEntry>::new();
        let mut saw_continue = false;
        let mut saw_push = false;
        let mut saw_complete = false;
        let mut saw_score_append = false;
        prop_assert!(!trace.driver_steps.is_empty());
        for step in &trace.driver_steps {
            prop_assert_eq!(step.score_len_before, prefix.len());
            match step.kind {
                rpb3_root_finish_trace::DriverStepKind::Continue => {
                    saw_continue = true;
                    prop_assert!(step.score_appended.is_empty());
                    prop_assert_eq!(step.score_len_after, step.score_len_before);
                }
                rpb3_root_finish_trace::DriverStepKind::Push(_) => {
                    saw_push = true;
                    prop_assert!(step.score_appended.is_empty());
                    prop_assert_eq!(step.score_len_after, step.score_len_before);
                }
                rpb3_root_finish_trace::DriverStepKind::Complete => {
                    saw_complete = true;
                    saw_score_append |= !step.score_appended.is_empty();
                }
            }
            prefix.extend(step.score_appended.iter().cloned());
            prop_assert_eq!(step.score_len_after, prefix.len());
            prop_assert!(prefix.len() <= final_entries.len());
            prop_assert_eq!(prefix.as_slice(), &final_entries[..prefix.len()]);
        }
        prop_assert!(saw_continue && saw_push && saw_complete && saw_score_append);
        prop_assert_eq!(prefix.as_slice(), final_entries.as_slice());
        prop_assert_eq!(trace.driver_steps.last().unwrap().after_len, 0);

        let transcripts = rpb3_root_finish_trace::replay_score_source(final_entries);
        let returned = trace.returned.as_ref().expect("generated Send returns a root summary");
        let root = returned.score_handle.expect("complete Send returns a score").index();
        prop_assert_eq!(root + 1, transcripts.len());
        prop_assert_eq!(transcripts[root].as_slice(), expected_score.as_slice());
        prop_assert_eq!(returned.score_bytes.as_deref(), Some(expected_score.as_slice()));
        prop_assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));

        prop_assert_eq!(trace.send_finishes.len(), 1);
        let finish = &trace.send_finishes[0];
        prop_assert!(finish.complete);
        prop_assert_eq!(finish.score_handle.index(), root);
        match &finish.input {
            SendFinishInput::Complete { channel, data, persistent: observed_persistent,
                connective_used, cost } => {
                prop_assert_eq!(*observed_persistent, persistent);
                prop_assert!(!*connective_used);
                prop_assert!(cost.is_none());
                prop_assert_eq!(data.len(), data_values.len());
                let expected_channel = expected_par_gint(channel_value);
                prop_assert_eq!(
                    transcripts[channel.index()].as_slice(),
                    expected_channel.as_slice(),
                );
                for (handle, value) in data.iter().zip(&data_values) {
                    let expected_data = expected_par_gint(*value);
                    prop_assert_eq!(
                        transcripts[handle.index()].as_slice(),
                        expected_data.as_slice(),
                    );
                }
            }
            SendFinishInput::Incomplete => prop_assert!(false, "generated Send finished incomplete"),
        }
    }
}
