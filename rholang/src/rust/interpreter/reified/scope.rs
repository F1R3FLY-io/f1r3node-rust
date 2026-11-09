//! The lexical use-context of the reducer (P4 design C.2, P8 design v2 AG-2 to AG-4).
//!
//! `ReducerCore` holds the immutable field `space_ctx`. `use s { P }` evaluates `P` with a
//! context that names the space `s`: a native space, or a user agent (P8). `with_metering_child`
//! clones the context into parallel branches. A body that the reducer stores for later
// Changed by decision 112: the wrapper of a stored body is a use block field. The text before:
// ```
// //! evaluation under a context is wrapped in a USE tuple, so the context returns when the body
// //! runs (a lexical closure).
// ```
//! evaluation under a context is wrapped in a use block (`Par.use_blocks`), so the context
//! returns when the body runs (a lexical closure).
//!
//! A user agent is named by its reference as the program evaluated it, with its bundle (AG-3).
//! A call on the agent is an ordinary send, so it follows the bundle rule of `eval_send`
//! ([`call_channel`]): a bundle with the write flag gives its body, and a bundle without it
//! fails. `use A` and `new x : A` apply the rule when they classify an agent (AG-4), so no
//! context and no agent-tagged name holds an agent that a call cannot reach.

use std::sync::Arc;

use models::rhoapi::{Bundle, Par};
use models::rust::reified::builders;
use models::rust::rholang::implicits::single_bundle;

use super::errors::ReifiedReduceError;
use crate::rust::interpreter::spaces::errors::SpaceReplyCode;
use crate::rust::interpreter::spaces::namespace::{self, NativeSpace, ReifiedId};

// Changed by P8 step 1 (design v2 AG-2): a `use` block names a native space or a user agent.
// /// The target of a `use` block: the space reference as the program evaluated it, and the
// /// native space that it names.
// #[derive(Clone, Debug, PartialEq)]
// pub struct UseTarget {
//     pub space: Par,
//     pub native: NativeSpace,
// }

/// What a `use` block names (P8, design v2 AG-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseKind {
    /// A native space.
    Native(NativeSpace),
    /// A user agent. Its reference is [`UseTarget::space`] (AG-3).
    Agent,
}

/// The target of a `use` block: the space reference as the program evaluated it, and what it
/// names.
#[derive(Clone, Debug, PartialEq)]
pub struct UseTarget {
    pub space: Par,
    pub kind: UseKind,
}

impl UseTarget {
    /// The native space of the target, or `None` for a user agent.
    pub fn native(&self) -> Option<NativeSpace> {
        match self.kind {
            UseKind::Native(space) => Some(space),
            UseKind::Agent => None,
        }
    }
}

/// The use-context of a reducer: `None` outside every `use` block.
pub type SpaceCtx = Option<Arc<UseTarget>>;

/// What a space reference names.
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceRef {
    /// A native space: its handle, bare or under a writable bundle.
    Native(NativeSpace),
    /// A value that is not a Reified id. A user agent implements the space (P8).
    Agent(Par),
}

/// A space reference that names no space. Each error is an abort in a `use` block or in a
/// `new` annotation, and a typed reply of `getSpaceAgent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceRefError {
    EmptySpaceRef,
    HandleNotWritable,
    NotASpaceRef,
}

impl SpaceRefError {
    /// The code of the typed reply of `getSpaceAgent`.
    pub const fn reply_code(self) -> SpaceReplyCode {
        match self {
            SpaceRefError::EmptySpaceRef => SpaceReplyCode::EmptySpaceRef,
            SpaceRefError::HandleNotWritable => SpaceReplyCode::HandleNotWritable,
            SpaceRefError::NotASpaceRef => SpaceReplyCode::NotASpaceRef,
        }
    }
}

impl From<SpaceRefError> for ReifiedReduceError {
    fn from(error: SpaceRefError) -> Self {
        match error {
            SpaceRefError::EmptySpaceRef => ReifiedReduceError::EmptySpaceRef,
            SpaceRefError::HandleNotWritable => ReifiedReduceError::HandleNotWritable,
            SpaceRefError::NotASpaceRef => ReifiedReduceError::NotASpaceRef,
        }
    }
}

fn is_empty(par: &Par) -> bool {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && exprs.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && use_blocks.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty()
}

/// Classifies the evaluated reference of `use s`, `new x : s` and `getSpaceAgent!?(s)`
/// (P4 design C.2). The function is total and deterministic.
///
/// 1. The empty `Par` is `EmptySpaceRef`.
/// 2. At most one bundle is removed, with the rule of `eval_send` (`single_bundle`).
/// 3. A handle id (`H` or `h`) is the native space when it is bare or under a writable
///    bundle. Under a bundle without the write flag it is `HandleNotWritable`.
/// 4. Any other Reified id is `NotASpaceRef`, so capabilities and storage ids are not agents.
/// 5. Every other value is an agent.
pub fn classify_space_ref(space: &Par) -> Result<SpaceRef, SpaceRefError> {
    if is_empty(space) {
        return Err(SpaceRefError::EmptySpaceRef);
    }
    let (value, writable) = match single_bundle(space) {
        Some(Bundle {
            body: Some(body),
            write_flag,
            ..
        }) => (body, write_flag),
        Some(Bundle {
            body: None,
            write_flag,
            ..
        }) => (Par::default(), write_flag),
        None => (space.clone(), true),
    };
    match (namespace::reified_id_of(&value), writable) {
        (Some(ReifiedId::Handle(native)), true) => Ok(SpaceRef::Native(native)),
        (Some(ReifiedId::Handle(_)), false) => Err(SpaceRefError::HandleNotWritable),
        (Some(_), _) => Err(SpaceRefError::NotASpaceRef),
        (None, _) => Ok(SpaceRef::Agent(space.clone())),
    }
}

/// The channel of a call on the user agent `agent` (P8, design v2 AG-4): the rule of
/// `eval_send` for a bundle. A bundle with the write flag gives its body, and a value that is no
/// bundle is its own channel. A bundle without the write flag, or without a body, gives
/// `HandleNotWritable`, the code of P4 that P8 generalizes to every space reference.
pub fn call_channel(agent: &Par) -> Result<Par, SpaceRefError> {
    match single_bundle(agent) {
        Some(Bundle {
            body: Some(body),
            write_flag: true,
            ..
        }) => Ok(body),
        Some(Bundle { .. }) => Err(SpaceRefError::HandleNotWritable),
        None => Ok(agent.clone()),
    }
}

// Changed by decision 112: the wrapper of a stored body is a use block field. The text before:
// ```
// /// Wraps a body that the reducer stores for later evaluation in the USE tuple of the
// /// context, `Par{exprs: [(USE, ctx.space, body)]}`. Without a context the body is unchanged.
// /// The wrapper is one term, so the reducer evaluates it with the random state of the body.
// ```
/// Wraps a body that the reducer stores for later evaluation in a use block of the context,
/// `Par{use_blocks: [UseBlock{ctx.space, body}]}`. Without a context the body is unchanged.
/// The wrapper is one term, so the reducer evaluates it with the random state of the body.
pub fn wrap_body(ctx: &SpaceCtx, body: Par) -> Par {
    match ctx {
        // Changed by decision 112: the wrapper of a stored body is a use block field. The text
        // before:
        // ```
        // Some(target) => builders::push_process_expr(
        //     Par::default(),
        //     builders::use_block(target.space.clone(), body),
        // ),
        // ```
        Some(target) => builders::push_use_block(
            Par::default(),
            builders::use_block(target.space.clone(), body),
        ),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    // Retired by decision 112: the tests read the use block field.
    // use models::rhoapi::expr::ExprInstance;
    // Changed by decision 112: the wrapper is a use block field. The text before:
    // use models::rust::reified::encoding::{decode_process_tuple, ProcessConstruct, UseBlock};
    use models::rust::reified::encoding::{decode_use_block, UseBlock};
    use models::rust::utils::{new_boundvar_par, new_bundle_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::spaces::namespace::{Durability, NameTail, SID_LEN};

    fn native(durability: Durability) -> NativeSpace {
        NativeSpace {
            sid: [8; SID_LEN],
            durability,
        }
    }

    fn handle(durability: Durability) -> Par {
        namespace::gprivate(namespace::handle_channel(native(durability)))
    }

    #[test]
    fn empty_reference_is_empty_space_ref() {
        assert_eq!(
            classify_space_ref(&Par::default()),
            Err(SpaceRefError::EmptySpaceRef)
        );
        let flagged = Par {
            connective_used: true,
            locally_free: vec![1],
            ..Par::default()
        };
        assert_eq!(
            classify_space_ref(&flagged),
            Err(SpaceRefError::EmptySpaceRef)
        );
    }

    #[test]
    fn bare_and_writable_handles_are_native_spaces() {
        for durability in [Durability::Durable, Durability::Ephemeral] {
            let expected = Ok(SpaceRef::Native(native(durability)));
            assert_eq!(classify_space_ref(&handle(durability)), expected);
            assert_eq!(
                classify_space_ref(&new_bundle_par(handle(durability), true, false)),
                expected
            );
            assert_eq!(
                classify_space_ref(&new_bundle_par(handle(durability), true, true)),
                expected
            );
        }
    }

    #[test]
    fn handle_under_a_bundle_without_write_flag_is_not_writable() {
        for read_flag in [false, true] {
            assert_eq!(
                classify_space_ref(&new_bundle_par(
                    handle(Durability::Durable),
                    false,
                    read_flag
                )),
                Err(SpaceRefError::HandleNotWritable)
            );
        }
    }

    #[test]
    fn other_reified_ids_are_not_space_refs_and_other_values_are_agents() {
        let space = native(Durability::Durable);
        for id in [
            namespace::config_channel(space),
            namespace::space_counter(space),
            namespace::name_id(space, NameTail::Random([1; 32])),
            namespace::op_channel(namespace::HANDLE_OP),
            namespace::op_channel(0x01),
        ] {
            let reference = namespace::gprivate(id);
            assert_eq!(
                classify_space_ref(&reference),
                Err(SpaceRefError::NotASpaceRef)
            );
            assert_eq!(
                classify_space_ref(&new_bundle_par(reference, true, false)),
                Err(SpaceRefError::NotASpaceRef)
            );
        }
        let nested = new_bundle_par(
            new_bundle_par(handle(Durability::Durable), true, false),
            true,
            false,
        );
        for value in [
            new_gstring_par("agent".to_string(), Vec::new(), false),
            namespace::gprivate(vec![7; SID_LEN]),
            nested,
        ] {
            assert_eq!(
                classify_space_ref(&value),
                Ok(SpaceRef::Agent(value.clone()))
            );
        }
        assert_eq!(
            SpaceRefError::HandleNotWritable.reply_code().code(),
            "HandleNotWritable"
        );
        assert_eq!(
            ReifiedReduceError::from(SpaceRefError::NotASpaceRef).code(),
            "NotASpaceRef"
        );
    }

    #[test]
    fn wrap_body_restores_the_context_as_a_use_tuple() {
        let body = new_boundvar_par(0, Vec::new(), false);
        assert_eq!(wrap_body(&None, body.clone()), body);
        let target = UseTarget {
            space: new_bundle_par(handle(Durability::Durable), true, false),
            kind: UseKind::Native(native(Durability::Durable)),
        };
        let wrapped = wrap_body(&Some(Arc::new(target.clone())), body.clone());
        // Changed by decision 112: the wrapper is a use block field. The text before:
        // ```
        // assert_eq!(wrapped.exprs.len(), 1);
        // assert_eq!(wrapped.locally_free, body.locally_free);
        // let Some(ExprInstance::ETupleBody(tuple)) = &wrapped.exprs[0].expr_instance else {
        //     panic!("the wrapper is not a tuple: {wrapped:?}");
        // };
        // assert_eq!(
        //     decode_process_tuple(tuple),
        //     Ok(ProcessConstruct::Use(UseBlock {
        //         space: target.space.clone(),
        //         body,
        //     }))
        // );
        // ```
        assert_eq!(wrapped.use_blocks.len(), 1);
        assert_eq!(wrapped.locally_free, body.locally_free);
        assert_eq!(
            decode_use_block(&wrapped.use_blocks[0]),
            Ok(UseBlock {
                space: target.space.clone(),
                body,
            })
        );
        assert_eq!(
            classify_space_ref(&target.space),
            Ok(SpaceRef::Native(native(Durability::Durable)))
        );
        assert_eq!(target.native(), Some(native(Durability::Durable)));
        let other = wrap_body(&Some(Arc::new(target)), new_gint_par(1, Vec::new(), false));
        assert_ne!(other, wrapped);
    }

    /// P8 (design v2 AG-4): the call channel of an agent follows the bundle rule of `eval_send`.
    #[test]
    fn agent_call_channel_follows_the_write_rule_of_eval_send() {
        let agent = new_gstring_par("agent".to_string(), Vec::new(), false);
        assert_eq!(call_channel(&agent), Ok(agent.clone()));
        for read_flag in [false, true] {
            assert_eq!(
                call_channel(&new_bundle_par(agent.clone(), true, read_flag)),
                Ok(agent.clone())
            );
            assert_eq!(
                call_channel(&new_bundle_par(agent.clone(), false, read_flag)),
                Err(SpaceRefError::HandleNotWritable)
            );
        }
        let no_body = Par::default().with_bundles(vec![Bundle {
            body: None,
            write_flag: true,
            read_flag: true,
        }]);
        assert_eq!(
            call_channel(&no_body),
            Err(SpaceRefError::HandleNotWritable)
        );
        // classify_space_ref keeps its P4 result for the same references (F12): the bundle stays
        // on the agent reference.
        let read_only = new_bundle_par(agent.clone(), false, true);
        assert_eq!(
            classify_space_ref(&read_only),
            Ok(SpaceRef::Agent(read_only.clone()))
        );
        let target = UseTarget {
            space: new_bundle_par(agent, true, true),
            kind: UseKind::Agent,
        };
        assert_eq!(target.native(), None);
        assert_eq!(
            ReifiedReduceError::from(SpaceRefError::HandleNotWritable).code(),
            "HandleNotWritable"
        );
    }
}
