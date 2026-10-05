//! The routing of a channel to a space (P4 design C.1, plan rules R1 to R7).
//!
//! The reducer routes a channel after `eval_send` or `unbundle_receive` has unbundled it. The
//! R7 backend routes the raw channel of a producer that bypasses the reducer, with no context.
//! A channel that a program can name without Reified syntax always routes to the default
//! space, so ordinary execution does not change.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{ETuple, Expr, GPrivate, GUnforgeable, Par};

use super::scope::SpaceCtx;
use crate::rust::interpreter::reduce::ReducerCore;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{
    self, CollectionKey, NativeSpace, ReifiedId, AGENT_TAG, REIFIED_TAG,
};

/// The class of a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelClass {
    /// A `GPrivate` with a 1-byte or 2-byte id: a byte name, an op channel or a marker.
    System,
    /// A config, handle, matcher or body id. These names live in the default space.
    DefaultObject,
    /// A data, path-index, allocator or counter id. These names are already routed.
    Storage,
    /// A name of a native space.
    SpaceName(NativeSpace),
    /// The tuple `(AGENT_TAG, A, raw)` of an agent-minted name (P8).
    AgentTagged,
    /// Any other unforgeable name.
    Unforgeable,
    /// Every other channel, a bundle included.
    Forged,
}

/// The space of a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// The default space: the host store, with the host semantics.
    Default,
    /// The collection `key` of a native space.
    Space {
        space: NativeSpace,
        key: CollectionKey,
    },
    /// A space that a user agent implements (P8).
    Agent,
}

fn sole_expr(par: &Par) -> Option<&ExprInstance> {
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
    let structural = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty();
    match (structural, exprs.as_slice()) {
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
        ) => Some(instance),
        (true, _) | (false, _) => None,
    }
}

fn is_agent_tagged(channel: &Par) -> bool {
    match sole_expr(channel) {
        Some(ExprInstance::ETupleBody(ETuple { ps, .. })) => match ps.as_slice() {
            [head, _, _] => {
                namespace::sole_gprivate_id(head) == Some(&[REIFIED_TAG, AGENT_TAG][..])
            }
            _ => false,
        },
        _ => false,
    }
}

/// Classifies a channel. The function is pure and total. It allocates nothing.
pub fn classify_channel(channel: &Par) -> ChannelClass {
    match namespace::sole_unforgeable(channel) {
        Some(GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }) => match (id.len(), namespace::decode_reified_id(id)) {
            (1 | 2, _) => ChannelClass::System,
            (
                _,
                Some(
                    ReifiedId::Config(_)
                    | ReifiedId::Handle(_)
                    | ReifiedId::Matcher(..)
                    | ReifiedId::Body(..),
                ),
            ) => ChannelClass::DefaultObject,
            (
                _,
                Some(
                    ReifiedId::Data(..)
                    | ReifiedId::PathIndex(..)
                    | ReifiedId::Allocator(_)
                    | ReifiedId::SpaceCounter(_)
                    | ReifiedId::CollectionCounter(..),
                ),
            ) => ChannelClass::Storage,
            (_, Some(ReifiedId::Name(space, _))) => ChannelClass::SpaceName(space),
            (_, Some(ReifiedId::Op(_) | ReifiedId::Reserved(..)) | None) => {
                ChannelClass::Unforgeable
            }
        },
        Some(_) => ChannelClass::Unforgeable,
        None if is_agent_tagged(channel) => ChannelClass::AgentTagged,
        None => ChannelClass::Forged,
    }
}

/// Routes a channel (P4 design C.1).
///
/// | Rule | Class                                   | Route                                  |
/// |------|-----------------------------------------|----------------------------------------|
/// | R1   | `System`, `DefaultObject`, `Unforgeable` | `Default`                             |
/// | R2   | `Storage`                               | `Default`                              |
/// | R3   | `SpaceName`                             | its space and `key(c)`, for every context |
/// | R4   | `AgentTagged`                           | `Agent`                                |
/// | R5   | `Forged`                                | the context space and `key(c)`, or `Default` without a context |
///
/// The key is computed only for a space route, so a default route allocates nothing.
pub fn route(ctx: &SpaceCtx, channel: &Par) -> Result<Route, SpaceError> {
    match (classify_channel(channel), ctx) {
        (
            ChannelClass::System
            | ChannelClass::DefaultObject
            | ChannelClass::Unforgeable
            | ChannelClass::Storage,
            _,
        ) => Ok(Route::Default),
        (ChannelClass::SpaceName(space), _) => Ok(Route::Space {
            space,
            key: CollectionKey::of_channel(channel)?,
        }),
        (ChannelClass::AgentTagged, _) => Ok(Route::Agent),
        (ChannelClass::Forged, Some(target)) => Ok(Route::Space {
            space: target.native,
            key: CollectionKey::of_channel(channel)?,
        }),
        (ChannelClass::Forged, None) => Ok(Route::Default),
    }
}

impl ReducerCore {
    /// Routes `channel` under the use-context of the reducer (the field `space_ctx`). The
    /// reducer hooks call it after the unbundling of `eval_send` and `unbundle_receive`.
    pub fn route_channel(&self, channel: &Par) -> Result<Route, SpaceError> {
        route(&self.space_ctx, channel)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::{BindPattern, GDeployId, ListParWithRandom, TaggedContinuation};
    use models::rust::utils::{new_bundle_par, new_etuple_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;

    use super::*;
    use crate::rust::interpreter::reified::scope::UseTarget;
    use crate::rust::interpreter::spaces::namespace::{Durability, Marker, NameTail, SID_LEN};
    use crate::rust::interpreter::system_processes::byte_name;
    use crate::rust::interpreter::test_utils::persistent_store_tester::create_test_space;

    fn native(seed: u8) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability: Durability::Durable,
        }
    }

    fn context(seed: u8) -> SpaceCtx {
        let space = native(seed);
        Some(Arc::new(UseTarget {
            space: namespace::gprivate(namespace::handle_channel(space)),
            native: space,
        }))
    }

    fn key(channel: &Par) -> CollectionKey { CollectionKey::of_channel(channel).expect("a key") }

    #[test]
    fn system_names_route_to_the_default_space() {
        let mut names = vec![byte_name(0), byte_name(37), byte_name(101)];
        names.extend(
            [
                0x01,
                namespace::GET_SPACE_AGENT,
                namespace::HANDLE_OP,
                AGENT_TAG,
            ]
            .into_iter()
            .map(|op| namespace::gprivate(namespace::op_channel(op))),
        );
        names.extend(Marker::ALL.into_iter().map(|marker| marker.par()));
        for name in names {
            assert_eq!(classify_channel(&name), ChannelClass::System, "{name:?}");
            for ctx in [None, context(1)] {
                assert_eq!(route(&ctx, &name), Ok(Route::Default));
            }
        }
    }

    #[test]
    fn default_objects_and_storage_route_to_the_default_space() {
        let collection = key(&new_gstring_par("c".to_string(), Vec::new(), false));
        for durability in [Durability::Durable, Durability::Ephemeral] {
            let space = NativeSpace {
                sid: [2; SID_LEN],
                durability,
            };
            for (id, class) in [
                (
                    namespace::config_channel(space),
                    ChannelClass::DefaultObject,
                ),
                (
                    namespace::handle_channel(space),
                    ChannelClass::DefaultObject,
                ),
                (namespace::space_counter(space), ChannelClass::Storage),
                (
                    namespace::collection_counter(space, &collection),
                    ChannelClass::Storage,
                ),
                (
                    namespace::data_channel(space, &collection),
                    ChannelClass::Storage,
                ),
            ] {
                let channel = namespace::gprivate(id);
                assert_eq!(classify_channel(&channel), class);
                for ctx in [None, context(3)] {
                    assert_eq!(route(&ctx, &channel), Ok(Route::Default));
                }
            }
        }
    }

    #[test]
    fn space_names_route_intrinsically() {
        let space = native(4);
        let name = namespace::gprivate(namespace::name_id(space, NameTail::Random([9; 32])));
        assert_eq!(classify_channel(&name), ChannelClass::SpaceName(space));
        let expected = Route::Space {
            space,
            key: key(&name),
        };
        for ctx in [None, context(4), context(5)] {
            assert_eq!(route(&ctx, &name), Ok(expected.clone()));
        }
    }

    #[test]
    fn forged_channels_route_to_the_context_space() {
        let forged = [
            new_gstring_par("x".to_string(), Vec::new(), false),
            new_gint_par(7, Vec::new(), false),
            new_bundle_par(
                namespace::gprivate(namespace::name_id(native(4), NameTail::Index(0))),
                true,
                false,
            ),
            Par::default(),
        ];
        for channel in forged {
            assert_eq!(
                classify_channel(&channel),
                ChannelClass::Forged,
                "{channel:?}"
            );
            assert_eq!(route(&None, &channel), Ok(Route::Default));
            assert_eq!(
                route(&context(6), &channel),
                Ok(Route::Space {
                    space: native(6),
                    key: key(&channel),
                })
            );
        }
    }

    #[test]
    fn agent_tagged_names_route_to_the_agent() {
        let tag = namespace::gprivate(namespace::op_channel(AGENT_TAG));
        let agent = new_gstring_par("agent".to_string(), Vec::new(), false);
        let raw = new_gint_par(1, Vec::new(), false);
        let tagged = new_etuple_par(vec![tag.clone(), agent.clone(), raw.clone()]);
        assert_eq!(classify_channel(&tagged), ChannelClass::AgentTagged);
        for ctx in [None, context(7)] {
            assert_eq!(route(&ctx, &tagged), Ok(Route::Agent));
        }
        let short = new_etuple_par(vec![tag, agent.clone()]);
        let other_head = new_etuple_par(vec![raw.clone(), agent, raw]);
        for channel in [short, other_head] {
            assert_eq!(classify_channel(&channel), ChannelClass::Forged);
        }
    }

    #[tokio::test]
    async fn reducer_routes_under_its_use_context() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let forged = new_gstring_par("x".to_string(), Vec::new(), false);
        assert_eq!(reducer.space_ctx, None);
        assert_eq!(reducer.route_channel(&forged), Ok(Route::Default));
        let mut core = ReducerCore::clone(&reducer.core);
        core.space_ctx = context(9);
        assert_eq!(
            core.route_channel(&forged),
            Ok(Route::Space {
                space: native(9),
                key: key(&forged),
            })
        );
    }

    #[test]
    fn ordinary_unforgeable_names_route_to_the_default_space() {
        let mut leading_tag = vec![7; SID_LEN];
        leading_tag[0] = REIFIED_TAG;
        let reserved = {
            let mut id = namespace::config_channel(native(1));
            id[1] = b'M';
            id
        };
        for channel in [
            namespace::gprivate(vec![7; SID_LEN]),
            namespace::gprivate(leading_tag),
            namespace::gprivate(reserved),
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![1, 2] })),
            }]),
        ] {
            assert_eq!(classify_channel(&channel), ChannelClass::Unforgeable);
            for ctx in [None, context(8)] {
                assert_eq!(route(&ctx, &channel), Ok(Route::Default));
            }
        }
    }
}
