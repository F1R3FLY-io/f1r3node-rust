//! The Reified system channels (P4 design A.5 and C.9) and the rule for a persistent send to
//! them (plan decision 32, A21, as readiness review §4 corrects it).
//!
//! | URN                             | Channel     | Arity | BodyRef | Phase      |
//! |---------------------------------|-------------|-------|---------|------------|
//! | `rho:space:hashmap:queue`       | `[E0, 01]`  | 2     | 201     | P4         |
//! | `rho:space:getSpaceAgent`       | `[E0, 21]`  | 2     | 233     | P4         |
//! | `rho:assert:assertEqual`        | `[E0, 22]`  | 3     | 234     | P4         |
//! | `rho:reified:internal:handleOp` | `[E0, 30]`  | 2     | 248     | P4         |
//! | the other families              | `[E0, 00..1F]` | 2  | 200 + op | P5 to P7  |
//! | `rho:lang:vector`               | `[E0, 23]`  | 2     | 235     | P6         |
//! | `rho:vectordb:embed`            | `[E0, 24]`  | 2     | 236     | P6         |
//! | `rho:vectordb:ann`              | `[E0, 25]`  | 5     | 237     | P6         |
//! | `rho:reified:internal:matcherOp` | `[E0, 31]` | 2     | 249     | P8         |
//! | `rho:reified:internal:sinkOp`   | `[E0, 32]`  | 0 and a remainder | 250 | P8  |
//!
//! The body ref of an op is `200 + op`. HEAD uses the body refs 0 to 36 and 101 to 108, and
//! 1-byte names, so the Reified ids cannot collide with them.

use models::rhoapi::Par;

use super::errors::ReifiedReduceError;
use crate::rust::interpreter::spaces::namespace::{
    self, NativeSpace, ReifiedId, ANN, ASSERT_EQUAL, EMBED, GET_SPACE_AGENT, HANDLE_OP, MATCHER_OP,
    NATIVE_ENGINE_FACTORY, SINK_OP, VECTOR,
};
use crate::rust::interpreter::system_processes::{Arity, BodyRef};

/// The first body ref of the Reified system processes.
pub const BODY_REF_BASE: BodyRef = 200;

/// The body ref of the op `op`: `200 + op`.
pub const fn body_ref(op: u8) -> BodyRef { BODY_REF_BASE + op as BodyRef }

/// A Reified system process: its URN, its op channel `[0xE0, op]` and its arity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemChannel {
    pub urn: &'static str,
    pub op: u8,
    pub arity: Arity,
}

impl SystemChannel {
    /// The body ref of the process.
    pub const fn body_ref(&self) -> BodyRef { body_ref(self.op) }

    /// The channel `GPrivate([0xE0, op])`.
    pub fn channel(&self) -> Par { namespace::gprivate(namespace::op_channel(self.op)) }
}

/// The factory of the family `hashmap:queue`: family index 1, `(ret, ctor)`.
pub const HASHMAP_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:queue",
    op: 0x01,
    arity: 2,
};

/// `getSpaceAgent!(ret, s)`.
pub const GET_SPACE_AGENT_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:space:getSpaceAgent",
    op: GET_SPACE_AGENT,
    arity: 2,
};

/// `assertEqual!(ret, a, b)` (plan decision 33).
pub const ASSERT_EQUAL_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:assert:assertEqual",
    op: ASSERT_EQUAL,
    arity: 3,
};

/// The internal handle process `(this, args)` (A6).
pub const HANDLE_OP_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:reified:internal:handleOp",
    op: HANDLE_OP,
    arity: 2,
};

/// The system processes that P4 registers.
pub const P4_SYSTEM_CHANNELS: [SystemChannel; 4] = [
    HASHMAP_QUEUE_FACTORY,
    GET_SPACE_AGENT_PROCESS,
    ASSERT_EQUAL_PROCESS,
    HANDLE_OP_PROCESS,
];

/// The op of a channel that is exactly the name `[0xE0, op]`.
pub fn op_of(channel: &Par) -> Option<u8> {
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Op(op)) => Some(op),
        _ => None,
    }
}

/// The space of a channel that is exactly a handle id (`H` or `h`).
pub fn handle_of(channel: &Par) -> Option<NativeSpace> {
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Handle(space)) => Some(space),
        _ => None,
    }
}

/// True for a factory op: the families `0x00..=0x1F` and the reserved engine factory `0x20`.
pub const fn is_factory_op(op: u8) -> bool { op <= NATIVE_ENGINE_FACTORY }

/// True for an op that takes calls: `0x00..=0x25` and `0x30..=0x32`. Markers and the agent tag
/// take no calls.
pub const fn is_call_op(op: u8) -> bool { op <= ANN || (HANDLE_OP <= op && op <= SINK_OP) }

/// A persistent send (`!!`) aborts on every op that takes calls and on every handle (A21),
/// and on every matcher and body capability of a translated receive (P8, design v2 A13). The
/// runtime never cancels the re-produce of a persistent send, so a call could repeat forever.
/// `channel` is the channel after the unbundling of `eval_send`.
pub fn check_persistent_send(channel: &Par, persistent: bool) -> Result<(), ReifiedReduceError> {
    if !persistent {
        return Ok(());
    }
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Op(op)) if is_factory_op(op) => {
            Err(ReifiedReduceError::PersistentFactoryCall)
        }
        Some(ReifiedId::Op(op)) if is_call_op(op) => Err(ReifiedReduceError::PersistentOpCall),
        Some(ReifiedId::Handle(_)) => Err(ReifiedReduceError::PersistentHandleCall),
        Some(ReifiedId::Matcher(..) | ReifiedId::Body(..)) => {
            Err(ReifiedReduceError::PersistentCapabilityCall)
        }
        _ => Ok(()),
    }
}

/// The factory of the family `hashmap:bag`: family index 0, `(ret, ctor)` (P5a).
pub const HASHMAP_BAG_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:bag",
    op: 0x00,
    arity: 2,
};

/// The factory of the family `hashmap:stack`: family index 2 (P5b).
pub const HASHMAP_STACK_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:stack",
    op: 0x02,
    arity: 2,
};

/// The factory of the family `hashmap:set`: family index 3 (P5a).
pub const HASHMAP_SET_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:set",
    op: 0x03,
    arity: 2,
};

/// The factory of the family `hashmap:cell`: family index 4 (P5a).
pub const HASHMAP_CELL_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:cell",
    op: 0x04,
    arity: 2,
};

/// The factory of the family `hashmap:priorityqueue`: family index 5 (P5b).
pub const HASHMAP_PRIORITY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:priorityqueue",
    op: 0x05,
    arity: 2,
};

/// The factory of the family `pathmap:bag`: family index 7 (P5e).
pub const PATHMAP_BAG_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:bag",
    op: 0x07,
    arity: 2,
};

/// The factory of the family `pathmap:queue`: family index 8 (P5e).
pub const PATHMAP_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:queue",
    op: 0x08,
    arity: 2,
};

/// The factory of the family `pathmap:stack`: family index 9 (P5e).
pub const PATHMAP_STACK_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:stack",
    op: 0x09,
    arity: 2,
};

/// The factory of the family `pathmap:set`: family index 10 (P5e).
pub const PATHMAP_SET_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:set",
    op: 0x0A,
    arity: 2,
};

/// The factory of the family `pathmap:cell`: family index 11 (P5e).
pub const PATHMAP_CELL_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:cell",
    op: 0x0B,
    arity: 2,
};

/// The factory of the family `pathmap:priorityqueue`: family index 12 (P5e).
pub const PATHMAP_PRIORITY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:pathmap:priorityqueue",
    op: 0x0C,
    arity: 2,
};

/// The factory of the family `array:bag`: family index 13 (P5d).
pub const ARRAY_BAG_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:bag",
    op: 0x0D,
    arity: 2,
};

/// The factory of the family `array:queue`: family index 14 (P5d).
pub const ARRAY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:queue",
    op: 0x0E,
    arity: 2,
};

/// The factory of the family `array:stack`: family index 15 (P5d).
pub const ARRAY_STACK_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:stack",
    op: 0x0F,
    arity: 2,
};

/// The factory of the family `array:set`: family index 16 (P5d).
pub const ARRAY_SET_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:set",
    op: 0x10,
    arity: 2,
};

/// The factory of the family `array:cell`: family index 17 (P5d).
pub const ARRAY_CELL_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:cell",
    op: 0x11,
    arity: 2,
};

/// The factory of the family `array:priorityqueue`: family index 18 (P5d).
pub const ARRAY_PRIORITY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:array:priorityqueue",
    op: 0x12,
    arity: 2,
};

/// The factory of the family `vector:bag`: family index 19 (P5d).
pub const VECTOR_BAG_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:bag",
    op: 0x13,
    arity: 2,
};

/// The factory of the family `vector:queue`: family index 20 (P5d).
pub const VECTOR_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:queue",
    op: 0x14,
    arity: 2,
};

/// The factory of the family `vector:stack`: family index 21 (P5d).
pub const VECTOR_STACK_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:stack",
    op: 0x15,
    arity: 2,
};

/// The factory of the family `vector:set`: family index 22 (P5d).
pub const VECTOR_SET_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:set",
    op: 0x16,
    arity: 2,
};

/// The factory of the family `vector:cell`: family index 23 (P5d).
pub const VECTOR_CELL_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:cell",
    op: 0x17,
    arity: 2,
};

/// The factory of the family `vector:priorityqueue`: family index 24 (P5d).
pub const VECTOR_PRIORITY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:priorityqueue",
    op: 0x18,
    arity: 2,
};

/// The factory of the family `hashmap:vectordb`: family index 6, `(ret, ctor)` (P6).
pub const HASHMAP_VECTORDB_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:vectordb",
    op: 0x06,
    arity: 2,
};

/// The factory of the family `vector:vectordb`: family index 25, `(ret, ctor)` (P6).
pub const VECTOR_VECTORDB_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:vector:vectordb",
    op: 0x19,
    arity: 2,
};

/// The FIPS alias `rho:space:HashMapBagSpace` of the family `hashmap:bag` (FIPS:296 and
/// 788-791, P5a). It names the factory channel of `hashmap:bag`, so the urn map binds the alias
/// to the capability of that family, and `getSpaceAgent` replies the same `Par` (plan decision
/// 9.4).
pub const HASHMAP_BAG_SPACE_ALIAS: SystemChannel = SystemChannel {
    urn: models::rust::space_factory_urn::HASHMAP_BAG_ALIAS,
    op: HASHMAP_BAG_FACTORY.op,
    arity: HASHMAP_BAG_FACTORY.arity,
};

/// The FIPS alias `rho:space:PathMapSpace` of the family `pathmap:bag` (FIPS:297, P5e). It names
/// the factory channel of `pathmap:bag`, as the alias of `hashmap:bag` names its family.
pub const PATHMAP_SPACE_ALIAS: SystemChannel = SystemChannel {
    urn: models::rust::space_factory_urn::PATHMAP_BAG_ALIAS,
    op: PATHMAP_BAG_FACTORY.op,
    arity: PATHMAP_BAG_FACTORY.arity,
};

/// The system processes that P5 adds after the four of P4, in the order of the definitions: the
/// factories in family-index order, then the aliases (P5 design v3, A.7, A.8 and A.10).
///
/// | URN                               | Channel           | Arity | BodyRef    | Sub-phase |
/// |-----------------------------------|-------------------|-------|------------|-----------|
/// | `rho:space:hashmap:bag`           | `[E0, 00]`        | 2     | 200        | P5a       |
/// | `rho:space:hashmap:stack`         | `[E0, 02]`        | 2     | 202        | P5b       |
/// | `rho:space:hashmap:set`           | `[E0, 03]`        | 2     | 203        | P5a       |
/// | `rho:space:hashmap:cell`          | `[E0, 04]`        | 2     | 204        | P5a       |
/// | `rho:space:hashmap:priorityqueue` | `[E0, 05]`        | 2     | 205        | P5b       |
/// | `rho:space:pathmap:<inner>`       | `[E0, 07..=0C]`   | 2     | 207 to 212 | P5e       |
/// | `rho:space:array:<inner>`         | `[E0, 0D..=12]`   | 2     | 213 to 218 | P5d       |
/// | `rho:space:vector:<inner>`        | `[E0, 13..=18]`   | 2     | 219 to 224 | P5d       |
/// | `rho:space:HashMapBagSpace`       | `[E0, 00]`        | 2     | 200        | P5a       |
/// | `rho:space:PathMapSpace`          | `[E0, 07]`        | 2     | 207        | P5e       |
///
/// The inner collections of a PathMap, an Array or a Vector store are bag, queue, stack, set,
/// cell and priority queue, in this order. The VectorDB families (6 and 25) are in
/// [`P6_SYSTEM_CHANNELS`], and the HashSet families (26 to 31) are in [`P7_SYSTEM_CHANNELS`]. A
/// process of this list is installed only when the family dispatch wires its family
/// (`spaces::factory::urn::registered_families`).
pub const P5_SYSTEM_CHANNELS: [SystemChannel; 25] = [
    HASHMAP_BAG_FACTORY,
    HASHMAP_STACK_FACTORY,
    HASHMAP_SET_FACTORY,
    HASHMAP_CELL_FACTORY,
    HASHMAP_PRIORITY_QUEUE_FACTORY,
    PATHMAP_BAG_FACTORY,
    PATHMAP_QUEUE_FACTORY,
    PATHMAP_STACK_FACTORY,
    PATHMAP_SET_FACTORY,
    PATHMAP_CELL_FACTORY,
    PATHMAP_PRIORITY_QUEUE_FACTORY,
    ARRAY_BAG_FACTORY,
    ARRAY_QUEUE_FACTORY,
    ARRAY_STACK_FACTORY,
    ARRAY_SET_FACTORY,
    ARRAY_CELL_FACTORY,
    ARRAY_PRIORITY_QUEUE_FACTORY,
    VECTOR_BAG_FACTORY,
    VECTOR_QUEUE_FACTORY,
    VECTOR_STACK_FACTORY,
    VECTOR_SET_FACTORY,
    VECTOR_CELL_FACTORY,
    VECTOR_PRIORITY_QUEUE_FACTORY,
    HASHMAP_BAG_SPACE_ALIAS,
    PATHMAP_SPACE_ALIAS,
];

/// The system processes that P6 adds after the processes of P5: the factories of the two
/// VectorDB families in family-index order (design v3 4.2).
///
/// | URN                          | Channel    | Arity | BodyRef |
/// |------------------------------|------------|-------|---------|
/// | `rho:space:hashmap:vectordb` | `[E0, 06]` | 2     | 206     |
/// | `rho:space:vector:vectordb`  | `[E0, 19]` | 2     | 225     |
///
/// They come after the processes of P4 and P5, so the definitions of P4 and P5 keep their
/// places. A process of this list is installed only when the family dispatch wires its family.
pub const P6_SYSTEM_CHANNELS: [SystemChannel; 2] =
    [HASHMAP_VECTORDB_FACTORY, VECTOR_VECTORDB_FACTORY];

/// `vec!(ret, op, x…)` (P6 step 6, design v3 4.7): the five operations of `rho:lang:vector`.
/// The reducer packs the arguments after `ret` into one list, as for a factory call, so the
/// process has the fixed arity 2 (I-18).
pub const VECTOR_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:lang:vector",
    op: VECTOR,
    arity: 2,
};

/// `embed!(ret, text)` or `embed!(ret, [text…])` (P6 step 7, decision 57): a recorded process.
/// Play embeds the texts with the embedder of the node, and replay produces the recorded reply.
pub const EMBED_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:vectordb:embed",
    op: EMBED,
    arity: 2,
};

/// The body ref of `rho:vectordb:embed`, in `non_deterministic_ops()`.
pub const EMBED_BODY_REF: BodyRef = body_ref(EMBED);

/// The body ref of `rho:vectordb:ann`, in `non_deterministic_ops()`.
pub const ANN_BODY_REF: BodyRef = body_ref(ANN);

/// `ann!(ret, space, channel, query, options)` (P6 step 7, decision 57): a recorded process.
/// Play asks the mirror of a VectorDB space for advisory ids, and replay produces the recorded
/// reply.
pub const ANN_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:vectordb:ann",
    op: ANN,
    arity: 5,
};

/// The system processes of P6 that are not factories, after the VectorDB factories, so every
/// earlier definition keeps its place.
///
/// | URN                  | Channel    | Arity | BodyRef |
/// |----------------------|------------|-------|---------|
/// | `rho:lang:vector`    | `[E0, 23]` | 2     | 235     |
/// | `rho:vectordb:embed` | `[E0, 24]` | 2     | 236     |
/// | `rho:vectordb:ann`   | `[E0, 25]` | 5     | 237     |
pub const P6_PROCESSES: [SystemChannel; 3] = [VECTOR_PROCESS, EMBED_PROCESS, ANN_PROCESS];

/// The factory of the family `hashset:bag`: family index 26, `(ret, ctor)` (P7).
pub const HASHSET_BAG_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:bag",
    op: 0x1A,
    arity: 2,
};

/// The factory of the family `hashset:queue`: family index 27 (P7).
pub const HASHSET_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:queue",
    op: 0x1B,
    arity: 2,
};

/// The factory of the family `hashset:stack`: family index 28 (P7).
pub const HASHSET_STACK_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:stack",
    op: 0x1C,
    arity: 2,
};

/// The factory of the family `hashset:set`: family index 29 (P7).
pub const HASHSET_SET_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:set",
    op: 0x1D,
    arity: 2,
};

/// The factory of the family `hashset:cell`: family index 30 (P7).
pub const HASHSET_CELL_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:cell",
    op: 0x1E,
    arity: 2,
};

/// The factory of the family `hashset:priorityqueue`: family index 31 (P7).
pub const HASHSET_PRIORITY_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashset:priorityqueue",
    op: 0x1F,
    arity: 2,
};

/// The system processes that P7 adds after the processes of P6: the factories of the six HashSet
/// families in family-index order (design v3 HS-7).
///
/// | URN                               | Channel    | Arity | BodyRef |
/// |-----------------------------------|------------|-------|---------|
/// | `rho:space:hashset:bag`           | `[E0, 1A]` | 2     | 226     |
/// | `rho:space:hashset:queue`         | `[E0, 1B]` | 2     | 227     |
/// | `rho:space:hashset:stack`         | `[E0, 1C]` | 2     | 228     |
/// | `rho:space:hashset:set`           | `[E0, 1D]` | 2     | 229     |
/// | `rho:space:hashset:cell`          | `[E0, 1E]` | 2     | 230     |
/// | `rho:space:hashset:priorityqueue` | `[E0, 1F]` | 2     | 231     |
///
/// They come after every process of P4 to P6, so every earlier definition keeps its place. A
/// process of this list is installed only when the family dispatch wires its family.
pub const P7_SYSTEM_CHANNELS: [SystemChannel; 6] = [
    HASHSET_BAG_FACTORY,
    HASHSET_QUEUE_FACTORY,
    HASHSET_STACK_FACTORY,
    HASHSET_SET_FACTORY,
    HASHSET_CELL_FACTORY,
    HASHSET_PRIORITY_QUEUE_FACTORY,
];

/// The internal matcher process `(m, args)` of the matcher capabilities of user agents (P8,
/// design v2 MO-1, plan decision 77 U1). The forwarder of a matcher capability sends it the
/// matcher id and the message `[ret, proc]` of the call.
pub const MATCHER_OP_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:reified:internal:matcherOp",
    op: MATCHER_OP,
    arity: 2,
};

/// The internal sink of the calls that a translation makes (P8, design v2 TS-2, plan decision
/// 77 U1). It has the arity 0 and a remainder, so it takes a reply of any arity and drops it.
pub const SINK_OP_PROCESS: SystemChannel = SystemChannel {
    urn: "rho:reified:internal:sinkOp",
    op: SINK_OP,
    arity: 0,
};

/// The system processes that P8 adds after the processes of P7, so every earlier definition
/// keeps its place (design v2 MO-1).
///
/// | URN                              | Channel    | Arity             | BodyRef |
/// |----------------------------------|------------|-------------------|---------|
/// | `rho:reified:internal:matcherOp` | `[E0, 31]` | 2                 | 249     |
/// | `rho:reified:internal:sinkOp`    | `[E0, 32]` | 0 and a remainder | 250     |
pub const P8_PROCESSES: [SystemChannel; 2] = [MATCHER_OP_PROCESS, SINK_OP_PROCESS];

/// True for an op whose call the reducer packs into `[ret, [x…]]` (I-18): every factory, and
/// `rho:lang:vector`.
pub const fn packs_arguments(op: u8) -> bool { is_factory_op(op) || op == VECTOR }

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use models::rust::utils::new_gstring_par;

    use super::*;
    use crate::rust::interpreter::spaces::namespace::{
        Durability, Marker, NameTail, AGENT_TAG, FACTORY_LAST, MATCHER_OP, SID_LEN,
    };
    use crate::rust::interpreter::system_processes::{byte_name, BodyRefs};

    #[test]
    fn p4_system_channels_have_the_table_values() {
        let expected = [
            ("rho:space:hashmap:queue", 0x01, 2, 201),
            ("rho:space:getSpaceAgent", 0x21, 2, 233),
            ("rho:assert:assertEqual", 0x22, 3, 234),
            ("rho:reified:internal:handleOp", 0x30, 2, 248),
        ];
        for (channel, (urn, op, arity, body)) in P4_SYSTEM_CHANNELS.iter().zip(expected) {
            assert_eq!(
                (channel.urn, channel.op, channel.arity, channel.body_ref()),
                (urn, op, arity, body)
            );
            assert_eq!(op_of(&channel.channel()), Some(op));
        }
        assert_eq!(body_ref(VECTOR), 235);
        assert_eq!(body_ref(MATCHER_OP), 249);
        assert_eq!(body_ref(SINK_OP), 250);
    }

    #[test]
    fn p5_system_channels_have_the_table_values() {
        let expected = [
            ("rho:space:hashmap:bag", 0x00, 2, 200),
            ("rho:space:hashmap:stack", 0x02, 2, 202),
            ("rho:space:hashmap:set", 0x03, 2, 203),
            ("rho:space:hashmap:cell", 0x04, 2, 204),
            ("rho:space:hashmap:priorityqueue", 0x05, 2, 205),
            ("rho:space:pathmap:bag", 0x07, 2, 207),
            ("rho:space:pathmap:queue", 0x08, 2, 208),
            ("rho:space:pathmap:stack", 0x09, 2, 209),
            ("rho:space:pathmap:set", 0x0A, 2, 210),
            ("rho:space:pathmap:cell", 0x0B, 2, 211),
            ("rho:space:pathmap:priorityqueue", 0x0C, 2, 212),
            ("rho:space:array:bag", 0x0D, 2, 213),
            ("rho:space:array:queue", 0x0E, 2, 214),
            ("rho:space:array:stack", 0x0F, 2, 215),
            ("rho:space:array:set", 0x10, 2, 216),
            ("rho:space:array:cell", 0x11, 2, 217),
            ("rho:space:array:priorityqueue", 0x12, 2, 218),
            ("rho:space:vector:bag", 0x13, 2, 219),
            ("rho:space:vector:queue", 0x14, 2, 220),
            ("rho:space:vector:stack", 0x15, 2, 221),
            ("rho:space:vector:set", 0x16, 2, 222),
            ("rho:space:vector:cell", 0x17, 2, 223),
            ("rho:space:vector:priorityqueue", 0x18, 2, 224),
            ("rho:space:HashMapBagSpace", 0x00, 2, 200),
            ("rho:space:PathMapSpace", 0x07, 2, 207),
        ];
        assert_eq!(P5_SYSTEM_CHANNELS.len(), expected.len());
        for (channel, (urn, op, arity, body)) in P5_SYSTEM_CHANNELS.iter().zip(expected) {
            assert_eq!(
                (channel.urn, channel.op, channel.arity, channel.body_ref()),
                (urn, op, arity, body)
            );
            assert_eq!(op_of(&channel.channel()), Some(op));
            assert!(is_factory_op(channel.op), "{}", channel.urn);
            assert!(is_call_op(channel.op), "{}", channel.urn);
        }
        let urns: BTreeSet<&str> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .map(|channel| channel.urn)
            .collect();
        assert_eq!(
            urns.len(),
            P4_SYSTEM_CHANNELS.len() + P5_SYSTEM_CHANNELS.len(),
            "every URN names one process"
        );
        let aliases = 2;
        let families = &P5_SYSTEM_CHANNELS[..P5_SYSTEM_CHANNELS.len() - aliases];
        let factory_ops: BTreeSet<u8> = families
            .iter()
            .chain(std::iter::once(&HASHMAP_QUEUE_FACTORY))
            .map(|channel| channel.op)
            .collect();
        assert_eq!(factory_ops.len(), families.len() + 1, "one op per family");
        for (alias, family, urn) in [
            (
                HASHMAP_BAG_SPACE_ALIAS,
                HASHMAP_BAG_FACTORY,
                models::rust::space_factory_urn::HASHMAP_BAG_ALIAS,
            ),
            (
                PATHMAP_SPACE_ALIAS,
                PATHMAP_BAG_FACTORY,
                models::rust::space_factory_urn::PATHMAP_BAG_ALIAS,
            ),
        ] {
            assert_eq!(
                alias.channel(),
                family.channel(),
                "the alias shares the channel of its family"
            );
            assert_eq!(alias.body_ref(), family.body_ref());
            assert_eq!(alias.urn, urn);
        }
    }

    #[test]
    fn p6_system_channels_have_the_table_values() {
        let expected = [
            ("rho:space:hashmap:vectordb", 0x06, 2, 206),
            ("rho:space:vector:vectordb", 0x19, 2, 225),
        ];
        assert_eq!(P6_SYSTEM_CHANNELS.len(), expected.len());
        for (channel, (urn, op, arity, body)) in P6_SYSTEM_CHANNELS.iter().zip(expected) {
            assert_eq!(
                (channel.urn, channel.op, channel.arity, channel.body_ref()),
                (urn, op, arity, body)
            );
            assert_eq!(op_of(&channel.channel()), Some(op));
            assert!(is_factory_op(channel.op), "{}", channel.urn);
            assert!(is_call_op(channel.op), "{}", channel.urn);
        }
        let urns: BTreeSet<&str> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .chain(P6_SYSTEM_CHANNELS.iter())
            .map(|channel| channel.urn)
            .collect();
        assert_eq!(
            urns.len(),
            P4_SYSTEM_CHANNELS.len() + P5_SYSTEM_CHANNELS.len() + P6_SYSTEM_CHANNELS.len(),
            "every URN names one process"
        );
        let earlier: BTreeSet<u8> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .map(|channel| channel.op)
            .collect();
        assert!(
            P6_SYSTEM_CHANNELS
                .iter()
                .all(|channel| !earlier.contains(&channel.op)),
            "a VectorDB factory has an op of its own"
        );
    }

    #[test]
    fn p7_system_channels_have_the_table_values() {
        let expected = [
            ("rho:space:hashset:bag", 0x1A, 2, 226),
            ("rho:space:hashset:queue", 0x1B, 2, 227),
            ("rho:space:hashset:stack", 0x1C, 2, 228),
            ("rho:space:hashset:set", 0x1D, 2, 229),
            ("rho:space:hashset:cell", 0x1E, 2, 230),
            ("rho:space:hashset:priorityqueue", 0x1F, 2, 231),
        ];
        assert_eq!(P7_SYSTEM_CHANNELS.len(), expected.len());
        for (channel, (urn, op, arity, body)) in P7_SYSTEM_CHANNELS.iter().zip(expected) {
            assert_eq!(
                (channel.urn, channel.op, channel.arity, channel.body_ref()),
                (urn, op, arity, body)
            );
            assert_eq!(op_of(&channel.channel()), Some(op));
            assert!(is_factory_op(channel.op), "{}", channel.urn);
            assert!(is_call_op(channel.op), "{}", channel.urn);
            assert!(packs_arguments(channel.op), "{}", channel.urn);
        }
        let earlier: Vec<&SystemChannel> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .chain(P6_SYSTEM_CHANNELS.iter())
            .chain(P6_PROCESSES.iter())
            .collect();
        let urns: BTreeSet<&str> = earlier
            .iter()
            .map(|channel| channel.urn)
            .chain(P7_SYSTEM_CHANNELS.iter().map(|channel| channel.urn))
            .collect();
        assert_eq!(
            urns.len(),
            earlier.len() + P7_SYSTEM_CHANNELS.len(),
            "every URN names one process"
        );
        let ops: BTreeSet<u8> = earlier.iter().map(|channel| channel.op).collect();
        assert!(
            P7_SYSTEM_CHANNELS
                .iter()
                .all(|channel| !ops.contains(&channel.op)),
            "a HashSet factory has an op of its own"
        );
        // The 32 families now cover every factory op below the engine factory.
        let families: BTreeSet<u8> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .chain(P6_SYSTEM_CHANNELS.iter())
            .chain(P7_SYSTEM_CHANNELS.iter())
            .map(|channel| channel.op)
            .filter(|op| is_factory_op(*op))
            .collect();
        assert_eq!(families, (0x00..=0x1F).collect::<BTreeSet<u8>>());
    }

    #[test]
    fn p6_processes_have_the_table_values() {
        let expected = [
            ("rho:lang:vector", 0x23, 2, 235),
            ("rho:vectordb:embed", 0x24, 2, 236),
            ("rho:vectordb:ann", 0x25, 5, 237),
        ];
        assert_eq!(P6_PROCESSES.len(), expected.len());
        for (channel, (urn, op, arity, body)) in P6_PROCESSES.iter().zip(expected) {
            assert_eq!(
                (channel.urn, channel.op, channel.arity, channel.body_ref()),
                (urn, op, arity, body)
            );
            assert_eq!(op_of(&channel.channel()), Some(op));
            assert!(!is_factory_op(channel.op), "{}", channel.urn);
            assert!(is_call_op(channel.op), "{}", channel.urn);
            assert_eq!(packs_arguments(channel.op), op == VECTOR, "{}", channel.urn);
        }
        let earlier: BTreeSet<u8> = P4_SYSTEM_CHANNELS
            .iter()
            .chain(P5_SYSTEM_CHANNELS.iter())
            .chain(P6_SYSTEM_CHANNELS.iter())
            .map(|channel| channel.op)
            .collect();
        assert!(
            P6_PROCESSES
                .iter()
                .all(|channel| !earlier.contains(&channel.op)),
            "a P6 process has an op of its own"
        );
        let packing: Vec<u8> = (0..=u8::MAX).filter(|op| packs_arguments(*op)).collect();
        let expected: Vec<u8> = (0x00..=0x20).chain([0x23]).collect();
        assert_eq!(
            packing, expected,
            "the factories, the engine factory and rho:lang:vector pack their arguments"
        );
    }

    #[test]
    fn body_refs_do_not_collide_with_head() {
        let head = [
            BodyRefs::STDOUT,
            BodyRefs::STDOUT_ACK,
            BodyRefs::STDERR,
            BodyRefs::STDERR_ACK,
            BodyRefs::ED25519_VERIFY,
            BodyRefs::SHA256_HASH,
            BodyRefs::KECCAK256_HASH,
            BodyRefs::BLAKE2B256_HASH,
            BodyRefs::SECP256K1_VERIFY,
            BodyRefs::GET_BLOCK_DATA,
            BodyRefs::GET_INVALID_BLOCKS,
            BodyRefs::VAULT_ADDRESS,
            BodyRefs::DEPLOYER_ID_OPS,
            BodyRefs::REG_OPS,
            BodyRefs::SYS_AUTHTOKEN_OPS,
            BodyRefs::REG_OPS_V1,
            BodyRefs::GPT4,
            BodyRefs::DALLE3,
            BodyRefs::TEXT_TO_AUDIO,
            BodyRefs::GRPC_TELL,
            BodyRefs::DEV_NULL,
            BodyRefs::ABORT,
            BodyRefs::OLLAMA_CHAT,
            BodyRefs::OLLAMA_GENERATE,
            BodyRefs::OLLAMA_MODELS,
            BodyRefs::DEPLOY_DATA,
            BodyRefs::CHROMA_CREATE_COLLECTION,
            BodyRefs::CHROMA_GET_COLLECTION_META,
            BodyRefs::CHROMA_UPSERT_ENTRIES,
            BodyRefs::CHROMA_QUERY,
            BodyRefs::CHROMA_DELETE_DOCUMENTS,
            BodyRefs::REGISTRY_LOOKUP,
        ];
        assert!(head.iter().all(|body| (0..=36).contains(body)));
        let reified: BTreeSet<BodyRef> = (0..=u8::MAX)
            .filter(|op| is_call_op(*op))
            .map(body_ref)
            .collect();
        assert!(reified.iter().all(|body| *body >= BODY_REF_BASE));
        assert!(reified.iter().all(|body| !(101..=108).contains(body)));
        let urns: BTreeSet<&str> = P4_SYSTEM_CHANNELS
            .iter()
            .map(|channel| channel.urn)
            .collect();
        assert_eq!(urns.len(), P4_SYSTEM_CHANNELS.len());
    }

    #[test]
    fn persistent_sends_abort_on_every_call_op_and_every_handle() {
        for op in 0..=u8::MAX {
            let channel = namespace::gprivate(namespace::op_channel(op));
            assert_eq!(check_persistent_send(&channel, false), Ok(()));
            let expected = match op {
                0x00..=FACTORY_LAST | NATIVE_ENGINE_FACTORY => {
                    Err(ReifiedReduceError::PersistentFactoryCall)
                }
                GET_SPACE_AGENT | ASSERT_EQUAL | VECTOR | EMBED | ANN | HANDLE_OP | MATCHER_OP
                | SINK_OP => Err(ReifiedReduceError::PersistentOpCall),
                _ => Ok(()),
            };
            assert_eq!(
                check_persistent_send(&channel, true),
                expected,
                "op {op:#04x}"
            );
        }
        for marker in Marker::ALL {
            assert_eq!(check_persistent_send(&marker.par(), true), Ok(()));
        }
        let agent_tag = namespace::gprivate(namespace::op_channel(AGENT_TAG));
        assert_eq!(check_persistent_send(&agent_tag, true), Ok(()));
        for durability in [Durability::Durable, Durability::Ephemeral] {
            let space = NativeSpace {
                sid: [3; SID_LEN],
                durability,
            };
            let this = namespace::gprivate(namespace::handle_channel(space));
            assert_eq!(handle_of(&this), Some(space));
            assert_eq!(check_persistent_send(&this, false), Ok(()));
            assert_eq!(
                check_persistent_send(&this, true),
                Err(ReifiedReduceError::PersistentHandleCall)
            );
            let name = namespace::gprivate(namespace::name_id(space, NameTail::Random([1; 32])));
            assert_eq!(check_persistent_send(&name, true), Ok(()));
        }
        // P8 (design v2 A13): a matcher or a body capability takes no persistent send, in its
        // uppercase and its lowercase form.
        for kind in [b'X', b'B', b'x', b'b'] {
            let mut id = vec![namespace::REIFIED_TAG, kind];
            id.extend_from_slice(&[4; SID_LEN]);
            let capability = namespace::gprivate(id);
            assert!(matches!(
                namespace::reified_id_of(&capability),
                Some(ReifiedId::Matcher(..) | ReifiedId::Body(..))
            ));
            assert_eq!(check_persistent_send(&capability, false), Ok(()));
            assert_eq!(
                check_persistent_send(&capability, true),
                Err(ReifiedReduceError::PersistentCapabilityCall),
                "kind {kind:#04x}"
            );
        }
        for ordinary in [
            byte_name(0),
            namespace::gprivate(vec![9; SID_LEN]),
            new_gstring_par("x".to_string(), Vec::new(), false),
        ] {
            assert_eq!(check_persistent_send(&ordinary, true), Ok(()));
            assert_eq!(op_of(&ordinary), None);
            assert_eq!(handle_of(&ordinary), None);
        }
    }
}
