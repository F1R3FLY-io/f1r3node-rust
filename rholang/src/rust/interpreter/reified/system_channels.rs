//! The Reified system channels (P4 design A.5 and C.9) and the rule for a persistent send to
//! them (plan decision 32, A21, as readiness review §4 corrects it).
//!
//! | URN                             | Channel     | Arity | BodyRef | Phase      |
//! |---------------------------------|-------------|-------|---------|------------|
//! | `rho:space:hashmap:queue`       | `[E0, 01]`  | 2     | 201     | P4         |
//! | `rho:space:getSpaceAgent`       | `[E0, 21]`  | 2     | 233     | P4         |
//! | `rho:assert:assertEqual`        | `[E0, 22]`  | 3     | 234     | P4         |
//! | `rho:reified:internal:handleOp` | `[E0, 30]`  | 2     | 248     | P4         |
//! | the other families              | `[E0, 00..1F]` | 2  | 200 + op | P5 and P6 |
//! | `rho:lang:vector`               | `[E0, 23]`  | —     | 235     | P6         |
//! | the matcher and the sink        | `[E0, 31]`, `[E0, 32]` | — | 249, 250 | P8 |
//!
//! The body ref of an op is `200 + op`. HEAD uses the body refs 0 to 36 and 101 to 108, and
//! 1-byte names, so the Reified ids cannot collide with them.

use models::rhoapi::Par;

use super::errors::ReifiedReduceError;
use crate::rust::interpreter::spaces::namespace::{
    self, NativeSpace, ReifiedId, ASSERT_EQUAL, GET_SPACE_AGENT, HANDLE_OP, NATIVE_ENGINE_FACTORY,
    SINK_OP, VECTOR,
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

/// True for an op that takes calls: `0x00..=0x23` and `0x30..=0x32`. Markers and the agent tag
/// take no calls.
pub const fn is_call_op(op: u8) -> bool { op <= VECTOR || (HANDLE_OP <= op && op <= SINK_OP) }

/// A persistent send (`!!`) aborts on every op that takes calls and on every handle (A21).
/// The runtime never cancels the re-produce of a persistent send, so a call could repeat
/// forever. `channel` is the channel after the unbundling of `eval_send`.
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
        _ => Ok(()),
    }
}

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
                GET_SPACE_AGENT | ASSERT_EQUAL | VECTOR | HANDLE_OP | MATCHER_OP | SINK_OP => {
                    Err(ReifiedReduceError::PersistentOpCall)
                }
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
