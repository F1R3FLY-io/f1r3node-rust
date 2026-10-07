// See models/src/main/scala/coop/rchain/models/ParSetTypeMapper.scala

use shared::rust::clone_backing::{BackingError, BackingMeter};

use super::par_set::ParSet;
use crate::rhoapi::ESet;

pub struct ParSetTypeMapper;

impl ParSetTypeMapper {
    pub fn eset_to_par_set(eset: ESet) -> ParSet {
        ParSet::new(
            eset.ps,
            eset.connective_used,
            eset.locally_free,
            eset.remainder,
        )
    }

    pub fn eset_to_par_set_metered(
        eset: ESet,
        meter: &dyn BackingMeter,
    ) -> Result<ParSet, BackingError> {
        ParSet::new_metered(
            eset.ps,
            eset.connective_used,
            eset.locally_free,
            eset.remainder,
            meter,
        )
    }

    pub fn par_set_to_eset(par_set: ParSet) -> ESet {
        ESet {
            ps: par_set.ps.sorted_pars,
            locally_free: par_set.locally_free,
            connective_used: par_set.connective_used,
            remainder: par_set.remainder,
        }
    }

    pub fn par_set_to_eset_prepaid(par_set: ParSet) -> ESet { Self::par_set_to_eset(par_set) }
}

#[cfg(test)]
mod metered_tests {
    use super::*;
    use crate::rust::utils::new_gint_par;

    #[test]
    fn metered_set_conversion_preserves_duplicate_element_semantics() {
        let first = new_gint_par(1, Vec::new(), false);
        let second = new_gint_par(2, Vec::new(), false);
        let eset = ESet {
            ps: vec![first.clone(), first, second],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let ordinary = ParSetTypeMapper::eset_to_par_set(eset.clone());
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let metered = ParSetTypeMapper::eset_to_par_set_metered(eset, &meter).unwrap();
        assert_eq!(metered.ps.sorted_pars, ordinary.ps.sorted_pars);
        assert_eq!(
            ParSetTypeMapper::par_set_to_eset_prepaid(metered).ps,
            ParSetTypeMapper::par_set_to_eset(ordinary).ps
        );
    }
}
