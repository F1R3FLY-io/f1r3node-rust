//! The space families that the runtime registers and their factory channels (P4 design A.5,
//! A9, and P5 design v3, A.7, A.8 and A.10).
//!
//! The factory of family `i` listens on the system channel `[0xE0, i]` with the body ref
//! `200 + i`. The urn map binds the URN of the family to the capability `bundle+{[0xE0, i]}`,
//! and `getSpaceAgent` replies with the same `Par`. A FIPS alias names the channel of its
//! family, so the urn map binds the alias to the same capability (plan decision 9.4).
//!
//! P4 registers `hashmap:queue` ([`REGISTERED_FAMILIES`]). P5 adds the rows of
//! [`P5_FAMILY_ROWS`] and [`P5_ALIAS_ROWS`]. A P5 family row is registered exactly when the
//! family dispatch wires its family (`families::Family`), so no factory creates a space that no
//! agent runs (F26). An alias row is registered exactly when its family is registered.
//! [`registered_families`] and [`registered_aliases`] give the registration. A family or an
//! alias without a registration has no URN binding and no installed process, so it fails
//! closed.

use models::rhoapi::Par;
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_bundle_par;

use crate::rust::interpreter::reified::system_channels::{
    SystemChannel, ARRAY_BAG_FACTORY, ARRAY_CELL_FACTORY, ARRAY_PRIORITY_QUEUE_FACTORY,
    ARRAY_QUEUE_FACTORY, ARRAY_SET_FACTORY, ARRAY_STACK_FACTORY, HASHMAP_BAG_FACTORY,
    HASHMAP_BAG_SPACE_ALIAS, HASHMAP_CELL_FACTORY, HASHMAP_PRIORITY_QUEUE_FACTORY,
    HASHMAP_QUEUE_FACTORY, HASHMAP_SET_FACTORY, HASHMAP_STACK_FACTORY, VECTOR_BAG_FACTORY,
    VECTOR_CELL_FACTORY, VECTOR_PRIORITY_QUEUE_FACTORY, VECTOR_QUEUE_FACTORY, VECTOR_SET_FACTORY,
    VECTOR_STACK_FACTORY,
};
use crate::rust::interpreter::spaces::families::Family;
use crate::rust::interpreter::spaces::namespace;

/// The families that P4 registers, with their factory system channels.
pub const REGISTERED_FAMILIES: [(SpaceFactoryUrn, SystemChannel); 1] =
    [(SpaceFactoryUrn::HASHMAP_QUEUE, HASHMAP_QUEUE_FACTORY)];

/// The family `outer:inner`.
const fn family(outer: SpaceOuter, inner: SpaceInner) -> SpaceFactoryUrn {
    SpaceFactoryUrn { outer, inner }
}

/// The families that P5 registers once the family dispatch wires them, with their factory
/// system channels, in family-index order: `hashmap` bag, stack, set, cell and priority queue
/// (P5a, P5b), then the six `array` families (13 to 18) and the six `vector` families (19 to
/// 24) of P5d.
pub const P5_FAMILY_ROWS: [(SpaceFactoryUrn, SystemChannel); 17] = [
    (
        family(SpaceOuter::HashMap, SpaceInner::Bag),
        HASHMAP_BAG_FACTORY,
    ),
    (
        family(SpaceOuter::HashMap, SpaceInner::Stack),
        HASHMAP_STACK_FACTORY,
    ),
    (
        family(SpaceOuter::HashMap, SpaceInner::Set),
        HASHMAP_SET_FACTORY,
    ),
    (
        family(SpaceOuter::HashMap, SpaceInner::Cell),
        HASHMAP_CELL_FACTORY,
    ),
    (
        family(SpaceOuter::HashMap, SpaceInner::PriorityQueue),
        HASHMAP_PRIORITY_QUEUE_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::Bag),
        ARRAY_BAG_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::Queue),
        ARRAY_QUEUE_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::Stack),
        ARRAY_STACK_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::Set),
        ARRAY_SET_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::Cell),
        ARRAY_CELL_FACTORY,
    ),
    (
        family(SpaceOuter::Array, SpaceInner::PriorityQueue),
        ARRAY_PRIORITY_QUEUE_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::Bag),
        VECTOR_BAG_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::Queue),
        VECTOR_QUEUE_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::Stack),
        VECTOR_STACK_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::Set),
        VECTOR_SET_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::Cell),
        VECTOR_CELL_FACTORY,
    ),
    (
        family(SpaceOuter::Vector, SpaceInner::PriorityQueue),
        VECTOR_PRIORITY_QUEUE_FACTORY,
    ),
];

/// The FIPS aliases that P5 registers once their family is registered: the system channel of
/// each alias, with the family that the alias names.
pub const P5_ALIAS_ROWS: [(SystemChannel, SpaceFactoryUrn); 1] = [(
    HASHMAP_BAG_SPACE_ALIAS,
    family(SpaceOuter::HashMap, SpaceInner::Bag),
)];

/// True when the family dispatch wires `family`, so an agent runs the spaces of the family.
pub fn is_wired(family: SpaceFactoryUrn) -> bool { Family::of_urn(family).is_some() }

/// The registered families of P5: the rows of [`P5_FAMILY_ROWS`] whose family is wired, in row
/// order.
pub fn registered_p5_families() -> impl Iterator<Item = (SpaceFactoryUrn, SystemChannel)> {
    P5_FAMILY_ROWS
        .into_iter()
        .filter(|(family, _)| is_wired(*family))
}

/// Every registered family with its factory system channel: the families of P4, then the
/// registered families of P5.
pub fn registered_families() -> impl Iterator<Item = (SpaceFactoryUrn, SystemChannel)> {
    REGISTERED_FAMILIES
        .into_iter()
        .chain(registered_p5_families())
}

/// The registered aliases: the rows of [`P5_ALIAS_ROWS`] whose family is registered, in row
/// order.
pub fn registered_aliases() -> impl Iterator<Item = (SystemChannel, SpaceFactoryUrn)> {
    P5_ALIAS_ROWS
        .into_iter()
        .filter(|(_, family)| registered_channel(*family).is_some())
}

/// The factory system channel of `family`, if the family is registered.
pub fn registered_channel(family: SpaceFactoryUrn) -> Option<SystemChannel> {
    registered_families()
        .find(|(registered, _)| *registered == family)
        .map(|(_, channel)| channel)
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
    use crate::rust::interpreter::reified::system_channels::{
        is_factory_op, op_of, P5_SYSTEM_CHANNELS,
    };
    use crate::rust::interpreter::spaces::factory::defaults::constructor_schema;

    /// The row of `family` in the tables of P4 and P5, registered or not.
    fn row_of(family: SpaceFactoryUrn) -> Option<SystemChannel> {
        REGISTERED_FAMILIES
            .iter()
            .chain(P5_FAMILY_ROWS.iter())
            .find(|(candidate, _)| *candidate == family)
            .map(|(_, channel)| *channel)
    }

    #[test]
    fn space_urn_factory_registered_families_match_their_system_channels() {
        for (family, channel) in REGISTERED_FAMILIES.into_iter().chain(P5_FAMILY_ROWS) {
            assert_eq!(channel.urn, family.to_string());
            assert_eq!(Some(channel.op), family.family_index());
            assert_eq!(channel.arity, 2);
            assert_eq!(factory_channel(family), Some(channel.channel()));
            assert!(is_factory_op(channel.op));
            assert!(constructor_schema(family).is_some(), "{family}");
        }
        for family in SpaceFactoryUrn::all() {
            let row = row_of(family);
            assert_eq!(
                row.is_some(),
                constructor_schema(family).is_some(),
                "{family}: a family has a row exactly when it has a schema"
            );
            assert_eq!(
                registered_channel(family),
                row.filter(|_| is_wired(family)),
                "{family}: a row is registered exactly when its family is wired"
            );
        }
        assert!(is_wired(SpaceFactoryUrn::HASHMAP_QUEUE));
        assert_eq!(
            registered_channel(SpaceFactoryUrn::HASHMAP_QUEUE),
            Some(HASHMAP_QUEUE_FACTORY)
        );
        assert_eq!(
            P5_FAMILY_ROWS.map(|(_, channel)| channel),
            P5_SYSTEM_CHANNELS[..P5_FAMILY_ROWS.len()],
            "the rows follow the order of the system channels"
        );
        let families: Vec<SpaceFactoryUrn> =
            registered_families().map(|(family, _)| family).collect();
        let distinct: std::collections::BTreeSet<SpaceFactoryUrn> =
            families.iter().copied().collect();
        assert_eq!(distinct.len(), families.len(), "no family registers twice");
        assert_eq!(families.first(), Some(&SpaceFactoryUrn::HASHMAP_QUEUE));
    }

    #[test]
    fn space_urn_factory_aliases_name_the_channel_of_their_family() {
        assert_eq!(
            P5_ALIAS_ROWS.map(|(channel, _)| channel),
            P5_SYSTEM_CHANNELS[P5_FAMILY_ROWS.len()..]
        );
        for (channel, family) in P5_ALIAS_ROWS {
            assert_eq!(SpaceFactoryUrn::parse(channel.urn), Ok(family));
            assert_ne!(channel.urn, family.to_string());
            assert_eq!(Some(channel.op), family.family_index());
            assert_eq!(channel.arity, 2);
            assert_eq!(Some(channel.channel()), factory_channel(family));
            assert_eq!(
                factory_capability(family),
                Some(new_bundle_par(channel.channel(), true, false)),
                "the alias binds the capability of its family"
            );
            assert_eq!(
                registered_aliases().any(|(registered, _)| registered == channel),
                registered_channel(family).is_some(),
                "an alias is registered exactly when its family is registered"
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
