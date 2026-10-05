//! The lexical use-context of the reducer (P4 design C.2).
//!
//! `ReducerCore` holds the immutable field `space_ctx`. `use s { P }` evaluates `P` with a
//! context that names the space `s`. `with_metering_child` clones the context into parallel
//! branches. A body that the reducer stores for later evaluation under a context is wrapped in
//! a USE tuple, so the context returns when the body runs (a lexical closure).

use std::sync::Arc;

use models::rhoapi::{Bundle, Par};
use models::rust::reified::builders;
use models::rust::rholang::implicits::single_bundle;

use super::errors::ReifiedReduceError;
use crate::rust::interpreter::spaces::errors::SpaceReplyCode;
use crate::rust::interpreter::spaces::namespace::{self, NativeSpace, ReifiedId};

/// The target of a `use` block: the space reference as the program evaluated it, and the
/// native space that it names.
#[derive(Clone, Debug, PartialEq)]
pub struct UseTarget {
    pub space: Par,
    pub native: NativeSpace,
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

/// Wraps a body that the reducer stores for later evaluation in the USE tuple of the
/// context, `Par{exprs: [(USE, ctx.space, body)]}`. Without a context the body is unchanged.
/// The wrapper is one term, so the reducer evaluates it with the random state of the body.
pub fn wrap_body(ctx: &SpaceCtx, body: Par) -> Par {
    match ctx {
        Some(target) => builders::push_process_expr(
            Par::default(),
            builders::use_block(target.space.clone(), body),
        ),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rust::reified::encoding::{decode_process_tuple, ProcessConstruct, UseBlock};
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
            native: native(Durability::Durable),
        };
        let wrapped = wrap_body(&Some(Arc::new(target.clone())), body.clone());
        assert_eq!(wrapped.exprs.len(), 1);
        assert_eq!(wrapped.locally_free, body.locally_free);
        let Some(ExprInstance::ETupleBody(tuple)) = &wrapped.exprs[0].expr_instance else {
            panic!("the wrapper is not a tuple: {wrapped:?}");
        };
        assert_eq!(
            decode_process_tuple(tuple),
            Ok(ProcessConstruct::Use(UseBlock {
                space: target.space.clone(),
                body,
            }))
        );
        assert_eq!(
            classify_space_ref(&target.space),
            Ok(SpaceRef::Native(target.native))
        );
        let other = wrap_body(&Some(Arc::new(target)), new_gint_par(1, Vec::new(), false));
        assert_ne!(other, wrapped);
    }
}
