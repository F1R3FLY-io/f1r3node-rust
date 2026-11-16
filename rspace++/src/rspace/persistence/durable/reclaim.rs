//! Authenticated, restart-safe superseded-generation reclamation records.

use super::catalog::CatalogSlotV1;
use crate::rspace::persistence::checksum::{crc32c, decode_crc32c, encode_crc32c};
use crate::rspace::persistence::retention::GenerationLeaseKeyV1;

const MAGIC: &[u8; 8] = b"RSPRECL1";
const VERSION: u64 = 1;
const BODY_LEN: usize = 8 + 8 + (8 * 8);
pub(crate) const RECLAMATION_RECORD_LEN_V1: usize = BODY_LEN + 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReclamationCandidateV1 {
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReclamationCatalogRelationV1 {
    PreCas,
    SelectedSuccessor,
    SelectedDescendant,
    Unrelated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReclamationUnitV1 {
    pub(crate) first_pack: u64,
    pub(crate) active_pack: u64,
    pub(crate) index_generation: u64,
}

fn same_physical_generation(left: GenerationLeaseKeyV1, right: GenerationLeaseKeyV1) -> bool {
    left.first_live_pack() == right.first_live_pack() &&
        left.active_pack() == right.active_pack() &&
        left.index_generation() == right.index_generation()
}

fn lockstep_distance(
    successor: GenerationLeaseKeyV1,
    selected: GenerationLeaseKeyV1,
) -> Option<u64> {
    let packs = selected
        .first_live_pack()
        .checked_sub(successor.active_pack())?;
    let indexes = selected
        .index_generation()
        .checked_sub(successor.index_generation())?;
    (selected.epoch() > successor.epoch() &&
        selected.first_live_pack() == selected.active_pack() &&
        packs > 0 &&
        packs == indexes)
        .then_some(packs)
}

impl ReclamationCandidateV1 {
    pub(crate) fn new(
        old: GenerationLeaseKeyV1,
        successor: GenerationLeaseKeyV1,
    ) -> Result<Self, &'static str> {
        if old.epoch() >= successor.epoch() || old.active_pack() >= successor.first_live_pack() {
            return Err("reclamation successor does not strictly supersede old generation");
        }
        Ok(Self { old, successor })
    }

    pub(crate) const fn old(self) -> GenerationLeaseKeyV1 { self.old }
    pub(crate) const fn successor(self) -> GenerationLeaseKeyV1 { self.successor }

    pub(crate) fn relation_to_key(
        self,
        selected: GenerationLeaseKeyV1,
    ) -> ReclamationCatalogRelationV1 {
        if selected.epoch() >= self.successor.epoch() &&
            same_physical_generation(selected, self.successor)
        {
            ReclamationCatalogRelationV1::SelectedSuccessor
        } else if selected.epoch() >= self.old.epoch() &&
            same_physical_generation(selected, self.old)
        {
            ReclamationCatalogRelationV1::PreCas
        } else if lockstep_distance(self.successor, selected).is_some() {
            ReclamationCatalogRelationV1::SelectedDescendant
        } else {
            ReclamationCatalogRelationV1::Unrelated
        }
    }

    pub(crate) fn relation_to_catalog(
        self,
        selected: CatalogSlotV1,
    ) -> ReclamationCatalogRelationV1 {
        self.relation_to_key(GenerationLeaseKeyV1::from_catalog(selected))
    }

    pub(crate) fn units_below(self, selected: GenerationLeaseKeyV1) -> Option<u64> {
        match self.relation_to_key(selected) {
            ReclamationCatalogRelationV1::SelectedSuccessor => Some(1),
            ReclamationCatalogRelationV1::SelectedDescendant => {
                lockstep_distance(self.successor, selected)?.checked_add(1)
            }
            ReclamationCatalogRelationV1::PreCas | ReclamationCatalogRelationV1::Unrelated => None,
        }
    }

    pub(crate) fn unit(self, number: u64) -> Option<ReclamationUnitV1> {
        match number {
            0 => Some(ReclamationUnitV1 {
                first_pack: self.old.first_live_pack(),
                active_pack: self.old.active_pack(),
                index_generation: self.old.index_generation(),
            }),
            1 => Some(ReclamationUnitV1 {
                first_pack: self.successor.first_live_pack(),
                active_pack: self.successor.active_pack(),
                index_generation: self.successor.index_generation(),
            }),
            later => {
                let advance = later.checked_sub(1)?;
                let pack = self.successor.active_pack().checked_add(advance)?;
                let index_generation = self.successor.index_generation().checked_add(advance)?;
                Some(ReclamationUnitV1 {
                    first_pack: pack,
                    active_pack: pack,
                    index_generation,
                })
            }
        }
    }

    pub(crate) fn unit_of_pack(self, pack: u64, below: u64) -> Option<u64> {
        let number = if (self.old.first_live_pack()..=self.old.active_pack()).contains(&pack) {
            0
        } else if (self.successor.first_live_pack()..=self.successor.active_pack()).contains(&pack)
        {
            1
        } else {
            pack.checked_sub(self.successor.active_pack())?
                .checked_add(1)?
        };
        (number < below).then_some(number)
    }

    pub(crate) fn unit_of_index(self, index_generation: u64, below: u64) -> Option<u64> {
        let number = if index_generation == self.old.index_generation() {
            0
        } else if index_generation == self.successor.index_generation() {
            1
        } else {
            index_generation
                .checked_sub(self.successor.index_generation())?
                .checked_add(1)?
        };
        (number < below).then_some(number)
    }

    pub(crate) fn from_catalogs(
        old: CatalogSlotV1,
        successor: CatalogSlotV1,
    ) -> Result<Self, &'static str> {
        Self::new(
            GenerationLeaseKeyV1::from_catalog(old),
            GenerationLeaseKeyV1::from_catalog(successor),
        )
    }

    pub(crate) fn encode(self) -> [u8; RECLAMATION_RECORD_LEN_V1] {
        let mut bytes = [0u8; RECLAMATION_RECORD_LEN_V1];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&VERSION.to_le_bytes());
        let fields = [
            self.old.epoch(),
            self.old.first_live_pack(),
            self.old.active_pack(),
            self.old.index_generation(),
            self.successor.epoch(),
            self.successor.first_live_pack(),
            self.successor.active_pack(),
            self.successor.index_generation(),
        ];
        for (index, value) in fields.into_iter().enumerate() {
            let start = 16 + index * 8;
            bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        let checksum = crc32c(&bytes[..BODY_LEN]);
        bytes[BODY_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != RECLAMATION_RECORD_LEN_V1 || &bytes[..8] != MAGIC {
            return Err("invalid reclamation record header");
        }
        if u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != VERSION {
            return Err("unsupported reclamation record version");
        }
        let stored = decode_crc32c(bytes[BODY_LEN..].try_into().unwrap())
            .map_err(|_| "reclamation record checksum is out of range")?;
        if stored != crc32c(&bytes[..BODY_LEN]) {
            return Err("reclamation record checksum mismatch");
        }
        let mut fields = [0u64; 8];
        for (index, field) in fields.iter_mut().enumerate() {
            let start = 16 + index * 8;
            *field = u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap());
        }
        Self::new(
            GenerationLeaseKeyV1::new(fields[0], fields[1], fields[2], fields[3]),
            GenerationLeaseKeyV1::new(fields[4], fields[5], fields[6], fields[7]),
        )
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn key(epoch: u64, first: u64, active: u64, index: u64) -> GenerationLeaseKeyV1 {
        GenerationLeaseKeyV1::new(epoch, first, active, index)
    }

    #[test]
    fn record_round_trips_and_rejects_tampering() {
        let candidate = ReclamationCandidateV1::new(key(1, 2, 3, 4), key(2, 4, 5, 6)).unwrap();
        let bytes = candidate.encode();
        assert_eq!(ReclamationCandidateV1::decode(&bytes), Ok(candidate));
        let mut altered = bytes;
        altered[24] ^= 1;
        assert_eq!(
            ReclamationCandidateV1::decode(&altered),
            Err("reclamation record checksum mismatch")
        );
    }

    #[test]
    fn record_rejects_non_superseding_generation() {
        assert_eq!(
            ReclamationCandidateV1::new(key(2, 2, 3, 4), key(2, 4, 5, 6)),
            Err("reclamation successor does not strictly supersede old generation")
        );
    }

    #[test]
    fn lockstep_descendants_number_the_units_below_them() {
        let candidate = ReclamationCandidateV1::new(key(4, 1, 3, 7), key(6, 4, 5, 8))
            .expect("the successor supersedes the old generation");
        for distance in 1u64..=4 {
            let selected = key(7 + distance, 5 + distance, 5 + distance, 8 + distance);
            assert_eq!(
                candidate.relation_to_key(selected),
                ReclamationCatalogRelationV1::SelectedDescendant
            );
            assert_eq!(candidate.units_below(selected), Some(distance + 1));
            let highest = candidate
                .unit(distance)
                .expect("the unit directly below the selected catalog exists");
            assert_eq!(highest.active_pack + 1, selected.first_live_pack());
            assert_eq!(highest.index_generation + 1, selected.index_generation());
        }
        assert_eq!(
            candidate.unit(0),
            Some(ReclamationUnitV1 {
                first_pack: 1,
                active_pack: 3,
                index_generation: 7,
            })
        );
        assert_eq!(
            candidate.unit(1),
            Some(ReclamationUnitV1 {
                first_pack: 4,
                active_pack: 5,
                index_generation: 8,
            })
        );
        assert_eq!(
            candidate.unit(2),
            Some(ReclamationUnitV1 {
                first_pack: 6,
                active_pack: 6,
                index_generation: 9,
            })
        );
        assert_eq!(candidate.unit(u64::MAX), None);
        assert_eq!(candidate.units_below(key(6, 4, 5, 8)), Some(1));
        assert_eq!(candidate.units_below(key(9, 4, 5, 8)), Some(1));
        assert_eq!(candidate.units_below(key(4, 1, 3, 7)), None);
    }

    #[test]
    fn catalogs_outside_the_lockstep_chain_stay_unrelated() {
        let candidate = ReclamationCandidateV1::new(key(4, 1, 3, 7), key(6, 4, 5, 8))
            .expect("the successor supersedes the old generation");
        for (shape, catalog) in [
            ("two packs for one index generation", key(9, 7, 7, 9)),
            ("two index generations for one pack", key(9, 6, 6, 10)),
            ("two live packs beyond the successor", key(9, 6, 7, 9)),
            ("a pack that overlaps the successor", key(9, 5, 6, 9)),
            ("the epoch of the successor", key(6, 6, 6, 9)),
            ("an epoch below the successor", key(5, 6, 6, 9)),
        ] {
            assert_eq!(
                candidate.relation_to_key(catalog),
                ReclamationCatalogRelationV1::Unrelated,
                "{shape}"
            );
            assert_eq!(candidate.units_below(catalog), None, "{shape}");
        }
    }

    #[test]
    fn ids_outside_the_units_below_the_selected_catalog_map_to_no_unit() {
        let candidate = ReclamationCandidateV1::new(key(4, 2, 3, 7), key(6, 6, 7, 10))
            .expect("the successor supersedes the old generation");
        let below = candidate
            .units_below(key(9, 10, 10, 13))
            .expect("the selected catalog is a lockstep descendant");
        assert_eq!(below, 4);
        for (pack, unit) in [
            (1, None),
            (2, Some(0)),
            (3, Some(0)),
            (4, None),
            (5, None),
            (6, Some(1)),
            (7, Some(1)),
            (8, Some(2)),
            (9, Some(3)),
            (10, None),
            (11, None),
            (u64::MAX, None),
        ] {
            assert_eq!(candidate.unit_of_pack(pack, below), unit, "pack {pack}");
        }
        for (index_generation, unit) in [
            (6, None),
            (7, Some(0)),
            (8, None),
            (9, None),
            (10, Some(1)),
            (11, Some(2)),
            (12, Some(3)),
            (13, None),
            (14, None),
            (u64::MAX, None),
        ] {
            assert_eq!(
                candidate.unit_of_index(index_generation, below),
                unit,
                "index generation {index_generation}"
            );
        }
        assert_eq!(candidate.unit_of_pack(3, 1), Some(0));
        assert_eq!(candidate.unit_of_pack(6, 1), None);
        assert_eq!(candidate.unit_of_index(7, 1), Some(0));
        assert_eq!(candidate.unit_of_index(10, 1), None);
    }

    proptest! {
        #[test]
        fn selected_reclaim_obligation_follows_physical_generation_not_exact_epoch(
            first in 1u64..10_000,
            old_width in 0u64..8,
            new_width in 0u64..8,
            old_epoch in 1u64..10_000,
            epoch_gap in 1u64..16,
            advances in 0u64..16,
            index in 1u64..10_000,
            distance in 1u64..16,
        ) {
            let old_last = first + old_width;
            let successor_first = old_last + 1;
            let successor_last = successor_first + new_width;
            let old = key(old_epoch, first, old_last, index);
            let successor = key(
                old_epoch + epoch_gap,
                successor_first,
                successor_last,
                index + 1,
            );
            let candidate = ReclamationCandidateV1::new(old, successor).unwrap();
            let selected = key(
                successor.epoch() + advances,
                successor_first,
                successor_last,
                successor.index_generation(),
            );
            prop_assert_eq!(
                candidate.relation_to_key(selected),
                ReclamationCatalogRelationV1::SelectedSuccessor
            );
            prop_assert_eq!(
                candidate.relation_to_key(key(
                    old.epoch() + advances,
                    old.first_live_pack(),
                    old.active_pack(),
                    old.index_generation(),
                )),
                ReclamationCatalogRelationV1::PreCas
            );
            for unrelated in [
                key(selected.epoch(), first, successor_last, selected.index_generation()),
                key(selected.epoch(), successor_first, successor_last + 1, selected.index_generation()),
                key(selected.epoch(), successor_first, successor_last, index + 2),
                key(successor.epoch() - 1, successor_first, successor_last, selected.index_generation()),
            ] {
                prop_assert_eq!(
                    candidate.relation_to_key(unrelated),
                    ReclamationCatalogRelationV1::Unrelated
                );
            }
            let descendant = key(
                successor.epoch() + 1 + advances,
                successor_last + distance,
                successor_last + distance,
                successor.index_generation() + distance,
            );
            prop_assert_eq!(
                candidate.relation_to_key(descendant),
                ReclamationCatalogRelationV1::SelectedDescendant
            );
            prop_assert_eq!(candidate.units_below(selected), Some(1));
            let below = distance + 1;
            prop_assert_eq!(candidate.units_below(descendant), Some(below));
            for number in 0..below {
                let unit = candidate
                    .unit(number)
                    .expect("every unit below the selected catalog exists");
                prop_assert!(unit.active_pack < descendant.first_live_pack());
                prop_assert!(unit.index_generation < descendant.index_generation());
                for pack in unit.first_pack..=unit.active_pack {
                    prop_assert_eq!(candidate.unit_of_pack(pack, below), Some(number));
                }
                prop_assert_eq!(
                    candidate.unit_of_index(unit.index_generation, below),
                    Some(number)
                );
            }
            let highest = candidate
                .unit(distance)
                .expect("the unit directly below the selected catalog exists");
            prop_assert_eq!(highest.active_pack + 1, descendant.first_live_pack());
            prop_assert_eq!(highest.index_generation + 1, descendant.index_generation());
            prop_assert_eq!(candidate.unit_of_pack(descendant.active_pack(), below), None);
            prop_assert_eq!(candidate.unit_of_index(descendant.index_generation(), below), None);
            for unrelated in [
                key(
                    descendant.epoch(),
                    descendant.first_live_pack(),
                    descendant.active_pack(),
                    descendant.index_generation() + 1,
                ),
                key(
                    descendant.epoch(),
                    descendant.first_live_pack(),
                    descendant.active_pack() + 1,
                    descendant.index_generation(),
                ),
                key(
                    successor.epoch(),
                    descendant.first_live_pack(),
                    descendant.active_pack(),
                    descendant.index_generation(),
                ),
            ] {
                prop_assert_eq!(
                    candidate.relation_to_key(unrelated),
                    ReclamationCatalogRelationV1::Unrelated
                );
                prop_assert_eq!(candidate.units_below(unrelated), None);
            }
        }
    }
}
