//! The config of a native space (I-17, P4 design B.2).
//!
//! The config of a space is a canonical `Par` map, the second par of the config record on
//! `C(sid)`:
//!
//! ```text
//! {"spaceId": GByteArray(sid), "outer": "hashmap", "inner": "queue", "qualifier": "default",
//!  "params": {}, "theory": Nil}
//! ```
//!
//! The factory builds the map with [`SpaceConfig::to_par`], and the agent stages it in the
//! creation of the space. The record is immutable. A collection operation, a handle method and
//! `getSpaceAgent` read it. The decoder is strict: it accepts exactly the image of
//! [`SpaceConfig::to_par`], and the theory slot is `Nil` in P4 (free theories arrive in P7).

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMap, Expr, KeyValuePair, Par};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;

use super::id::space_id_from_bytes;
use super::qualifier::SpaceQualifier;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::SpaceId;
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr, MalformedRecord, RecordKind};

/// The key of the space id.
pub const SPACE_ID_KEY: &str = "spaceId";

/// The key of the outer store.
pub const OUTER_KEY: &str = "outer";

/// The key of the inner collection.
pub const INNER_KEY: &str = "inner";

/// The key of the qualifier.
pub const QUALIFIER_KEY: &str = "qualifier";

/// The key of the named factory params.
pub const PARAMS_KEY: &str = "params";

/// The key of the theory.
pub const THEORY_KEY: &str = "theory";

/// The number of entries of a config map.
pub const CONFIG_ENTRIES: usize = 6;

/// The config of a native space.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceConfig {
    /// The id of the space.
    pub space_id: SpaceId,
    /// The name of the outer store, for example `hashmap`.
    pub outer: String,
    /// The name of the inner collection, for example `queue`.
    pub inner: String,
    /// The qualifier of the space.
    pub qualifier: SpaceQualifier,
    /// The named factory params, by key.
    pub params: BTreeMap<String, Par>,
}

impl SpaceConfig {
    /// The config of a new space of the family `outer:inner` without params.
    pub fn new(space_id: SpaceId, outer: &str, inner: &str, qualifier: SpaceQualifier) -> Self {
        Self {
            space_id,
            outer: outer.to_owned(),
            inner: inner.to_owned(),
            qualifier,
            params: BTreeMap::new(),
        }
    }

    /// True when the space has the family `outer:inner`.
    pub fn is_family(&self, outer: &str, inner: &str) -> bool {
        self.outer == outer && self.inner == inner
    }

    /// The canonical config map (I-17). `ParMap` sorts its entries, and the bit sets follow the
    /// normalizer.
    pub fn to_par(&self) -> Par {
        let params = map_par(
            self.params
                .iter()
                .map(|(key, value)| (string_par(key), value.clone()))
                .collect(),
        );
        map_par(vec![
            (
                string_par(SPACE_ID_KEY),
                expr_par(ExprInstance::GByteArray(self.space_id.to_vec())),
            ),
            (string_par(OUTER_KEY), string_par(&self.outer)),
            (string_par(INNER_KEY), string_par(&self.inner)),
            (
                string_par(QUALIFIER_KEY),
                string_par(self.qualifier.as_str()),
            ),
            (string_par(PARAMS_KEY), params),
            (string_par(THEORY_KEY), Par::default()),
        ])
    }

    /// Decodes a config map. Anything but the exact image of [`SpaceConfig::to_par`] is a
    /// malformed config record: another key set, a value of another kind, a space id without
    /// 32 bytes, an unknown qualifier, a theory other than `Nil`, or another entry order or bit
    /// flag.
    pub fn from_par(map: &Par) -> Result<Self, MalformedRecord> {
        let malformed = MalformedRecord(RecordKind::Config);
        let entries = map_entries(map).ok_or(malformed)?;
        if entries.len() != CONFIG_ENTRIES {
            return Err(malformed);
        }
        let entry = |key: &str| entries.get(key).copied().ok_or(malformed);
        let config = SpaceConfig {
            space_id: bytes_of(entry(SPACE_ID_KEY)?)
                .and_then(space_id_from_bytes)
                .ok_or(malformed)?,
            outer: string_of(entry(OUTER_KEY)?).ok_or(malformed)?.to_owned(),
            inner: string_of(entry(INNER_KEY)?).ok_or(malformed)?.to_owned(),
            qualifier: string_of(entry(QUALIFIER_KEY)?)
                .and_then(SpaceQualifier::parse)
                .ok_or(malformed)?,
            params: map_entries(entry(PARAMS_KEY)?)
                .ok_or(malformed)?
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect(),
        };
        match (
            *entry(THEORY_KEY)? == Par::default(),
            config.to_par() == *map,
        ) {
            (true, true) => Ok(config),
            (true, false) | (false, _) => Err(malformed),
        }
    }
}

impl SpaceConfig {
    /// [`SpaceConfig::from_par`] with its work reserved on `meter` before the decode makes it
    /// (decision 42). The decode walks the entries of `map`, copies the decoded fields, encodes
    /// the decoded config again with [`SpaceConfig::to_par`] and compares the two maps. Each
    /// part reads or copies at most `map`, so the reservations are a read of `map`, two copies
    /// of `map` and a read of each side of the comparison.
    pub fn from_par_metered(map: &Par, meter: &ScanMeter) -> Result<Self, SpaceError> {
        meter.inspect(map)?;
        meter.reserve_copy(map)?;
        meter.reserve_copy(map)?;
        meter.inspect(map)?;
        meter.inspect(map)?;
        Ok(Self::from_par(map)?)
    }
}

pub(crate) fn string_par(value: &str) -> Par { expr_par(ExprInstance::GString(value.to_owned())) }

pub(crate) fn map_par(pairs: Vec<(Par, Par)>) -> Par {
    let map = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(pairs));
    Par {
        locally_free: map.locally_free.clone(),
        connective_used: map.connective_used,
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        }],
        ..Par::default()
    }
}

fn string_of(par: &Par) -> Option<&str> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GString(value)), false) => Some(value),
        _ => None,
    }
}

fn bytes_of(par: &Par) -> Option<&[u8]> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GByteArray(bytes)), false) => Some(bytes),
        _ => None,
    }
}

/// The entries of a `Par` that is exactly one map without a remainder whose keys are distinct
/// strings.
pub(crate) fn map_entries(par: &Par) -> Option<BTreeMap<&str, &Par>> {
    match sole_expr(par) {
        Some(ExprInstance::EMapBody(EMap {
            kvs,
            remainder: None,
            ..
        })) => {
            let mut entries = BTreeMap::new();
            for KeyValuePair { key, value } in kvs {
                let key = string_of(key.as_ref()?)?;
                if entries.insert(key, value.as_ref()?).is_some() {
                    return None;
                }
            }
            Some(entries)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::Var;
    use models::rust::utils::new_gint_par;

    use super::*;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;

    fn sid() -> SpaceId { [5; SID_LEN] }

    fn queue() -> SpaceConfig {
        SpaceConfig::new(sid(), "hashmap", "queue", SpaceQualifier::Default)
    }

    fn map_of(entries: Vec<(&str, Par)>) -> Par {
        map_par(
            entries
                .into_iter()
                .map(|(key, value)| (string_par(key), value))
                .collect(),
        )
    }

    fn i17(entries: Vec<(&str, Par)>) -> Vec<(&str, Par)> {
        let mut base = vec![
            (
                SPACE_ID_KEY,
                expr_par(ExprInstance::GByteArray(sid().to_vec())),
            ),
            (OUTER_KEY, string_par("hashmap")),
            (INNER_KEY, string_par("queue")),
            (QUALIFIER_KEY, string_par("default")),
            (PARAMS_KEY, map_par(Vec::new())),
            (THEORY_KEY, Par::default()),
        ];
        for (key, value) in entries {
            match base.iter_mut().find(|(existing, _)| *existing == key) {
                Some(entry) => entry.1 = value,
                None => base.push((key, value)),
            }
        }
        base
    }

    fn with_map(par: &Par, change: impl FnOnce(&mut EMap)) -> Par {
        let mut par = par.clone();
        if let Some(Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        }) = par.exprs.first_mut()
        {
            change(map);
        }
        par
    }

    #[test]
    fn space_config_map_has_the_i17_shape() {
        let map = queue().to_par();
        assert_eq!(map, map_of(i17(Vec::new())));
        let mut reversed = i17(Vec::new());
        reversed.reverse();
        assert_eq!(map, map_of(reversed));
        let entries = map_entries(&map).expect("a config map");
        assert_eq!(entries.len(), CONFIG_ENTRIES);
        assert_eq!(entries.keys().copied().collect::<Vec<_>>(), vec![
            INNER_KEY,
            OUTER_KEY,
            PARAMS_KEY,
            QUALIFIER_KEY,
            SPACE_ID_KEY,
            THEORY_KEY
        ]);
        assert_eq!(entries.get(THEORY_KEY), Some(&&Par::default()));
        assert!(!map.connective_used);
    }

    #[test]
    fn space_config_roundtrips_every_qualifier_and_params() {
        for qualifier in [
            SpaceQualifier::Default,
            SpaceQualifier::Temp,
            SpaceQualifier::Seq,
        ] {
            let config = SpaceConfig::new(sid(), "hashmap", "queue", qualifier);
            assert_eq!(SpaceConfig::from_par(&config.to_par()), Ok(config));
        }
        let mut config = queue();
        config
            .params
            .insert("priorities".to_string(), new_gint_par(3, Vec::new(), false));
        config.params.insert("cap".to_string(), string_par("x"));
        assert_eq!(SpaceConfig::from_par(&config.to_par()), Ok(config.clone()));
        assert_ne!(config.to_par(), queue().to_par());
        assert!(queue().is_family("hashmap", "queue"));
        assert!(!queue().is_family("hashmap", "bag"));
        assert!(!queue().is_family("pathmap", "queue"));
    }

    #[test]
    fn space_config_decoder_rejects_every_other_map() {
        let malformed = Err(MalformedRecord(RecordKind::Config));
        let canonical = queue().to_par();
        let mut missing = i17(Vec::new());
        missing.retain(|(key, _)| *key != THEORY_KEY);
        let mut renamed = i17(Vec::new());
        renamed.retain(|(key, _)| *key != INNER_KEY);
        renamed.push(("Inner", string_par("queue")));
        let reversed = with_map(&canonical, |map| map.kvs.reverse());
        let flagged = with_map(&canonical, |map| map.connective_used = true);
        let mut outer_flag = canonical.clone();
        outer_flag.connective_used = true;
        let remainder = with_map(&canonical, |map| {
            map.remainder = Some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            })
        });
        let duplicated = with_map(&canonical, |map| {
            let first = map.kvs[0].clone();
            map.kvs.push(first);
        });
        let keyless = with_map(&canonical, |map| map.kvs[0].key = None);
        for bad in [
            map_of(missing),
            map_of(renamed),
            map_of(i17(vec![("extra", Par::default())])),
            map_of(i17(vec![(
                SPACE_ID_KEY,
                expr_par(ExprInstance::GByteArray(vec![5; SID_LEN - 1])),
            )])),
            map_of(i17(vec![(SPACE_ID_KEY, string_par("sid"))])),
            map_of(i17(vec![(OUTER_KEY, new_gint_par(1, Vec::new(), false))])),
            map_of(i17(vec![(QUALIFIER_KEY, string_par("temporary"))])),
            map_of(i17(vec![(THEORY_KEY, string_par("Nat"))])),
            map_of(i17(vec![(PARAMS_KEY, Par::default())])),
            map_of(i17(vec![(
                PARAMS_KEY,
                map_par(vec![(Par::default(), Par::default())]),
            )])),
            reversed,
            flagged,
            outer_flag,
            remainder,
            duplicated,
            keyless,
            string_par("a config"),
            Par::default(),
        ] {
            assert_eq!(SpaceConfig::from_par(&bad), malformed, "{bad:?}");
        }
        assert_eq!(SpaceConfig::from_par(&canonical), Ok(queue()));
    }
}
