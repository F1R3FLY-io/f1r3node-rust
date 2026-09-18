use proptest::prelude::*;

use super::{checked_add, delimited_len, finish_node, EmitMode, Node, Piece, ProtobufRegionError};

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
