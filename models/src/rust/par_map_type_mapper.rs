// See models/src/main/scala/coop/rchain/models/ParMapTypeMapper.scala

use shared::rust::clone_backing::{BackingError, BackingMeter};

use super::par_map::ParMap;
use crate::rhoapi::{EMap, KeyValuePair, Par};

pub struct ParMapTypeMapper;

impl ParMapTypeMapper {
    pub fn emap_to_par_map(emap: EMap) -> ParMap {
        let ps: Vec<(Par, Par)> = emap.kvs.into_iter().map(Self::unzip).collect();
        ParMap::new(ps, emap.connective_used, emap.locally_free, emap.remainder)
    }

    pub fn emap_to_par_map_metered(
        emap: EMap,
        meter: &dyn BackingMeter,
    ) -> Result<ParMap, BackingError> {
        let entries = emap.kvs.len();
        let scanned = entries
            .checked_mul(std::mem::size_of::<KeyValuePair>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(entries, scanned, 0)?;
        if emap
            .kvs
            .iter()
            .any(|kvp| kvp.key.is_none() || kvp.value.is_none())
        {
            return Err(BackingError::Rejected);
        }
        let backing = entries
            .checked_mul(std::mem::size_of::<(Par, Par)>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(1, 0, backing)?;
        let mut ps = Vec::new();
        ps.try_reserve_exact(entries)
            .map_err(|_| BackingError::Allocation)?;
        for kvp in emap.kvs {
            ps.push((
                kvp.key.ok_or(BackingError::Rejected)?,
                kvp.value.ok_or(BackingError::Rejected)?,
            ));
        }
        ParMap::new_metered(
            ps,
            emap.connective_used,
            emap.locally_free,
            emap.remainder,
            meter,
        )
    }

    pub fn par_map_to_emap(par_map: ParMap) -> EMap {
        let kvs: Vec<KeyValuePair> = par_map
            .ps
            .sorted_list
            .iter()
            .map(|(k, v)| Self::zip(k.clone(), v.clone()))
            .collect();

        EMap {
            kvs,
            locally_free: par_map.locally_free,
            connective_used: par_map.connective_used,
            remainder: par_map.remainder,
        }
    }

    pub fn par_map_to_emap_metered(
        par_map: ParMap,
        meter: &dyn BackingMeter,
    ) -> Result<EMap, BackingError> {
        let entries = par_map.ps.sorted_list.len();
        let scanned = entries
            .checked_mul(std::mem::size_of::<(Par, Par)>())
            .ok_or(BackingError::Overflow)?;
        let backing = entries
            .checked_mul(std::mem::size_of::<KeyValuePair>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(entries, scanned, backing)?;
        let mut kvs = Vec::new();
        kvs.try_reserve_exact(entries)
            .map_err(|_| BackingError::Allocation)?;
        for (key, value) in par_map.ps.into_sorted_list_prepaid() {
            kvs.push(Self::zip(key, value));
        }
        Ok(EMap {
            kvs,
            locally_free: par_map.locally_free,
            connective_used: par_map.connective_used,
            remainder: par_map.remainder,
        })
    }

    fn unzip(kvp: KeyValuePair) -> (Par, Par) { (kvp.key.unwrap(), kvp.value.unwrap()) }

    fn zip(k: Par, v: Par) -> KeyValuePair {
        KeyValuePair {
            key: Some(k),
            value: Some(v),
        }
    }
}

#[cfg(test)]
mod metered_tests {
    use super::*;
    use crate::rust::utils::new_gint_par;

    #[test]
    fn metered_map_conversion_preserves_duplicate_key_semantics() {
        let key = new_gint_par(1, Vec::new(), false);
        let first = new_gint_par(2, Vec::new(), false);
        let last = new_gint_par(3, Vec::new(), false);
        let emap = EMap {
            kvs: vec![
                KeyValuePair {
                    key: Some(key.clone()),
                    value: Some(first),
                },
                KeyValuePair {
                    key: Some(key),
                    value: Some(last),
                },
            ],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let ordinary = ParMapTypeMapper::emap_to_par_map(emap.clone());
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let metered = ParMapTypeMapper::emap_to_par_map_metered(emap, &meter).unwrap();
        assert_eq!(metered.ps.sorted_list, ordinary.ps.sorted_list);
        assert_eq!(
            ParMapTypeMapper::par_map_to_emap_metered(metered, &meter)
                .unwrap()
                .kvs,
            ParMapTypeMapper::par_map_to_emap(ordinary).kvs
        );
    }

    #[test]
    fn metered_map_conversion_rejects_missing_key_or_value() {
        let meter = |_: usize, _: usize, _: usize| Ok(());
        for kvp in [
            KeyValuePair {
                key: None,
                value: Some(Par::default()),
            },
            KeyValuePair {
                key: Some(Par::default()),
                value: None,
            },
        ] {
            let emap = EMap {
                kvs: vec![kvp],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            };
            assert!(matches!(
                ParMapTypeMapper::emap_to_par_map_metered(emap, &meter),
                Err(BackingError::Rejected)
            ));
        }
    }
}
