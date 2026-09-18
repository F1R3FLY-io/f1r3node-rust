use super::CanonicalProtobufError;

pub(crate) fn scan(bytes: &[u8], at: &mut usize) -> Result<u64, CanonicalProtobufError> {
    super::read_canonical_varint(bytes, at)
}
