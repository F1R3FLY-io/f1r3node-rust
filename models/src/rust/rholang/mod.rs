pub mod grpc_client;
pub mod implicits;
pub mod par_children;
pub mod par_codec;
pub mod sorter;
pub mod wire;
pub mod wire_encode;

/// The GENERATED wire-schema table — one `impl WireNode` per message, one
/// `impl WireOneof` per oneof, and the oneof index tables — emitted by
/// `models/build/wire_schema.rs` from the protobuf `FileDescriptorSet`.
///
/// ★ ONE table, BOTH directions: [`wire_encode`] drives the serializer from
/// it and [`par_codec`] drives the deserializer from it. The variant counts
/// here are `.len()` of the generated tables, never literals, so a new oneof
/// arm cannot leave an assertion passing while it goes untested.
#[allow(clippy::all, unused_imports)]
pub mod wire_schema {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_wire.rs"));
}
