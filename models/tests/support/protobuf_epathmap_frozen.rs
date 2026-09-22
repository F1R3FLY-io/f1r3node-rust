use super::*;
#[allow(deprecated)]
pub(crate) fn merge_field(
    value: &mut EPathMap,
    tag: u32,
    protobuf_wire_type: WireType,
    buf: &mut impl Buf,
    ctx: DecodeContext,
) -> Result<(), DecodeError> {
    /// prost-derive parity: the error-context struct name pushed onto
    /// `DecodeError` paths.
    const STRUCT_NAME: &str = "EPathMap";
    match tag {
        1u32 => {
            let mut payload = Vec::new();
            encoding::bytes::merge(protobuf_wire_type, &mut payload, buf, ctx).map_err(
                |mut error| {
                    error.push(STRUCT_NAME, "ps");
                    error
                },
            )?;
            let par = crate::protobuf_decoder_oracle::frozen::decode_par(payload.as_slice())
                .map_err(|mut error| {
                    error.push(STRUCT_NAME, "ps");
                    error
                })?;
            value
                .ps
                .try_insert_entry(par)
                .map_err(|error| DecodeError::new(format!("EPathMap ps: {error}")))?;
            Ok(())
        }
        3u32 => {
            let value = &mut value.locally_free;
            encoding::bytes::merge(protobuf_wire_type, value, buf, ctx).map_err(|mut error| {
                error.push(STRUCT_NAME, "locally_free");
                error
            })
        }
        4u32 => {
            let value = &mut value.connective_used;
            encoding::bool::merge(protobuf_wire_type, value, buf, ctx).map_err(|mut error| {
                error.push(STRUCT_NAME, "connective_used");
                error
            })
        }
        5u32 => {
            let value = &mut value.remainder;
            encoding::message::merge(
                protobuf_wire_type,
                value.get_or_insert_with(Default::default),
                buf,
                ctx,
            )
            .map_err(|mut error| {
                error.push(STRUCT_NAME, "remainder");
                error
            })
        }
        8u32 => {
            // Legacy set-only key stream retained for backward reads.
            let mut region: Vec<u8> = Vec::new();
            encoding::bytes::merge(protobuf_wire_type, &mut region, buf, ctx).map_err(
                |mut error| {
                    error.push(STRUCT_NAME, "serialized_paths");
                    error
                },
            )?;
            value.merge_serialized_paths(&region)
        }
        9u32 => {
            let mut region: Vec<u8> = Vec::new();
            encoding::bytes::merge(protobuf_wire_type, &mut region, buf, ctx).map_err(
                |mut error| {
                    error.push(STRUCT_NAME, "trie_snapshot");
                    error
                },
            )?;
            value.replace_trie_snapshot(&region)
        }
        _ => {
            crate::protobuf_decoder_oracle::frozen::skip_unknown_field(protobuf_wire_type, tag, buf)
        }
    }
}
