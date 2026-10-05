//! The space families that the runtime registers and their factory channels (P4 design A.5,
//! A9).
//!
//! The factory of family `i` listens on the system channel `[0xE0, i]` with the body ref
//! `200 + i`. The urn map binds the URN of the family to the capability `bundle+{[0xE0, i]}`,
//! and `getSpaceAgent` replies with the same `Par`. P4 registers only `hashmap:queue`. The other
//! families have no URN binding and no installed process, so they fail closed until P5.

use models::rhoapi::Par;
use models::rust::space_factory_urn::SpaceFactoryUrn;
use models::rust::utils::new_bundle_par;

use crate::rust::interpreter::reified::system_channels::{SystemChannel, HASHMAP_QUEUE_FACTORY};
use crate::rust::interpreter::spaces::namespace;

/// The families that P4 registers, with their factory system channels.
pub const REGISTERED_FAMILIES: [(SpaceFactoryUrn, SystemChannel); 1] =
    [(SpaceFactoryUrn::HASHMAP_QUEUE, HASHMAP_QUEUE_FACTORY)];

/// The factory system channel of `family`, if this phase registers the family.
pub fn registered_channel(family: SpaceFactoryUrn) -> Option<SystemChannel> {
    REGISTERED_FAMILIES
        .iter()
        .find(|(registered, _)| *registered == family)
        .map(|(_, channel)| *channel)
}

/// The factory channel `[0xE0, i]` of a family with the index `i`.
pub fn factory_channel(family: SpaceFactoryUrn) -> Option<Par> {
    family
        .family_index()
        .map(|index| namespace::gprivate(namespace::op_channel(index)))
}

/// The factory capability of a family: `bundle+{[0xE0, i]}`. It equals the urn map value of
/// the URN of the family (`Definition::to_urn_map`).
pub fn factory_capability(family: SpaceFactoryUrn) -> Option<Par> {
    factory_channel(family).map(|channel| new_bundle_par(channel, true, false))
}

#[cfg(test)]
mod tests {
    use models::rhoapi::Bundle;

    use super::*;
    use crate::rust::interpreter::reified::system_channels::{is_factory_op, op_of};

    #[test]
    fn space_urn_factory_registered_families_match_their_system_channels() {
        for (family, channel) in REGISTERED_FAMILIES {
            assert_eq!(channel.urn, family.to_string());
            assert_eq!(Some(channel.op), family.family_index());
            assert_eq!(channel.arity, 2);
            assert_eq!(registered_channel(family), Some(channel));
            assert_eq!(factory_channel(family), Some(channel.channel()));
            assert!(is_factory_op(channel.op));
        }
        for family in SpaceFactoryUrn::all() {
            assert_eq!(
                registered_channel(family).is_some(),
                family == SpaceFactoryUrn::HASHMAP_QUEUE,
                "{family}"
            );
        }
    }

    #[test]
    fn space_urn_factory_capability_is_the_write_bundle_of_the_factory_channel() {
        for family in SpaceFactoryUrn::all() {
            let index = family.family_index().expect("a supported family");
            let channel = factory_channel(family).expect("a factory channel");
            assert_eq!(op_of(&channel), Some(index));
            assert_eq!(
                factory_capability(family),
                Some(Par::default().with_bundles(vec![Bundle {
                    body: Some(channel),
                    write_flag: true,
                    read_flag: false,
                }]))
            );
        }
        let capabilities: std::collections::BTreeSet<Vec<u8>> = SpaceFactoryUrn::all()
            .map(|family| {
                prost::Message::encode_to_vec(&factory_capability(family).expect("a capability"))
            })
            .collect();
        assert_eq!(capabilities.len(), SpaceFactoryUrn::all().count());
    }
}
