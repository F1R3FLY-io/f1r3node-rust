use super::*;

#[allow(deprecated)]
fn take_length_delimited_component<B: Buf>(
    wire_type: WireType,
    buf: &mut B,
) -> Result<prost::bytes::buf::Take<&mut B>, DecodeError> {
    encoding::check_wire_type(WireType::LengthDelimited, wire_type)?;
    let length = encoding::decode_varint(buf)?;
    if length > buf.remaining() as u64 {
        return Err(DecodeError::new("buffer underflow"));
    }
    Ok((&mut *buf).take(length as usize))
}

pub(crate) fn merge_field(
    parent: &mut Par,
    tag: u32,
    wire_type: WireType,
    buf: &mut impl Buf,
    ctx: DecodeContext,
) -> Result<(), DecodeError> {
    macro_rules! merge_component {
        ($component:ty, $decode:path) => {{
            let body = take_length_delimited_component(wire_type, buf)?;
            let value: $component = $decode(body)?;
            parent.merge_protobuf_component(&value)
        }};
    }

    match tag {
        1 => merge_component!(Send, crate::protobuf_decoder_oracle::frozen::decode_send),
        2 => merge_component!(
            Receive,
            crate::protobuf_decoder_oracle::frozen::decode_receive
        ),
        4 => merge_component!(New, crate::protobuf_decoder_oracle::frozen::decode_new),
        5 => {
            let mut body = take_length_delimited_component(wire_type, buf)?;
            let direct = (body.chunk().len() == body.remaining())
                .then(|| parent.merge_canonical_expr_protobuf_fast_path(body.chunk()))
                .flatten();
            if let Some(result) = direct {
                let remaining = body.remaining();
                body.advance(remaining);
                return result;
            }
            let value = crate::protobuf_decoder_oracle::frozen::decode_expr(body)?;
            parent.merge_protobuf_component(&value)
        }
        6 => merge_component!(Match, crate::protobuf_decoder_oracle::frozen::decode_match),
        7 => merge_component!(
            GUnforgeable,
            crate::protobuf_decoder_oracle::frozen::decode_g_unforgeable
        ),
        8 => merge_component!(
            Connective,
            crate::protobuf_decoder_oracle::frozen::decode_connective
        ),
        9 => parent.merge_protobuf_locally_free(wire_type, buf, ctx),
        10 => parent.merge_protobuf_connective_used(wire_type, buf, ctx),
        11 => merge_component!(
            Bundle,
            crate::protobuf_decoder_oracle::frozen::decode_bundle
        ),
        12 => merge_component!(If, crate::protobuf_decoder_oracle::frozen::decode_if),
        13 => merge_component!(
            CostTokenStack,
            crate::protobuf_decoder_oracle::frozen::decode_cost_token_stack
        ),
        _ => crate::protobuf_decoder_oracle::frozen::skip_unknown_field(wire_type, tag, buf),
    }
}
