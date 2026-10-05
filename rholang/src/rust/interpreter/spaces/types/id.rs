//! The id of a native space (I-19, P4 design B.8).
//!
//! A factory call derives the id of its space from its own random state:
//! `sid = random.split_byte(0xE0).split_byte(SPACE_ID).next()`, 32 bytes. The random state of a
//! call message is unique, so the id is unique to the call, and play and replay derive the same
//! id. The id type is `namespace::SpaceId`.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;

use crate::rust::interpreter::spaces::namespace::{RandomPurpose, SpaceId, SID_LEN};

/// The id of the space that a factory call with the random state `call` creates (I-19).
pub fn derive_space_id(call: &Blake2b512Random) -> SpaceId {
    let mut sid = [0u8; SID_LEN];
    for (byte, value) in sid
        .iter_mut()
        .zip(RandomPurpose::SpaceId.derive(call).next())
    {
        *byte = value as u8;
    }
    sid
}

/// The space id with the bytes `bytes`, or `None` when `bytes` does not have `SID_LEN` bytes.
pub fn space_id_from_bytes(bytes: &[u8]) -> Option<SpaceId> { SpaceId::try_from(bytes).ok() }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::spaces::namespace::REIFIED_TAG;

    #[test]
    fn space_id_is_the_space_id_purpose_draw_of_the_call() {
        let call = Blake2b512Random::create_from_bytes(b"factory call");
        let mut expected = call.split_byte(REIFIED_TAG).split_byte(0x01);
        let draw: Vec<u8> = expected.next().into_iter().map(|byte| byte as u8).collect();
        assert_eq!(draw.len(), SID_LEN);
        assert_eq!(derive_space_id(&call).to_vec(), draw);
        assert_eq!(derive_space_id(&call), derive_space_id(&call.clone()));
    }

    #[test]
    fn space_ids_of_distinct_calls_differ_and_do_not_consume_the_call() {
        let call = Blake2b512Random::create_from_bytes(b"factory call");
        let other = Blake2b512Random::create_from_bytes(b"another call");
        assert_ne!(derive_space_id(&call), derive_space_id(&other));
        assert_ne!(derive_space_id(&call), derive_space_id(&call.split_byte(1)));
        let mut untouched = call.clone();
        let mut reference = Blake2b512Random::create_from_bytes(b"factory call");
        let _ = derive_space_id(&call);
        assert_eq!(untouched.next(), reference.next());
    }

    #[test]
    fn space_id_from_bytes_takes_exactly_sid_len_bytes() {
        let sid = [7u8; SID_LEN];
        assert_eq!(space_id_from_bytes(&sid), Some(sid));
        assert_eq!(space_id_from_bytes(&sid[..SID_LEN - 1]), None);
        assert_eq!(space_id_from_bytes(&[7u8; SID_LEN + 1]), None);
        assert_eq!(space_id_from_bytes(&[]), None);
    }
}
