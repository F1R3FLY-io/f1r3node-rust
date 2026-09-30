// See models/src/main/scala/coop/rchain/models/rholang/sorter/VarSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::Var;

pub struct VarSortMatcher;

impl VarSortMatcher {
    pub fn sort_match_metered(
        v: &Var,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Var>, BackingError> {
        let score = match &v.var_instance {
            Some(VarInstance::BoundVar(level)) => Tree::<ScoreAtom>::create_node_from_i64s_metered(
                &[Score::BOUND_VAR as i64, i64::from(*level)],
                meter,
            )?,
            Some(VarInstance::FreeVar(level)) => Tree::<ScoreAtom>::create_node_from_i64s_metered(
                &[Score::FREE_VAR as i64, i64::from(*level)],
                meter,
            )?,
            Some(VarInstance::Wildcard(_)) => {
                Tree::<ScoreAtom>::create_node_from_i64s_metered(&[Score::WILDCARD as i64], meter)?
            }
            None => Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        };
        let term = if v.var_instance.is_some() {
            meter.clone(v)?
        } else {
            Var::default()
        };
        Ok(ScoredTerm { term, score })
    }
}

impl Sortable<Var> for VarSortMatcher {
    fn sort_match(v: &Var) -> ScoredTerm<Var> {
        match &v.var_instance {
            Some(var) => match var {
                VarInstance::BoundVar(level) => ScoredTerm {
                    term: v.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::BOUND_VAR as i64,
                        *level as i64,
                    ]),
                },

                VarInstance::FreeVar(level) => ScoredTerm {
                    term: v.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::FREE_VAR as i64,
                        *level as i64,
                    ]),
                },

                VarInstance::Wildcard(_) => ScoredTerm {
                    term: v.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::WILDCARD as i64]),
                },
            },
            None => ScoredTerm {
                term: Var::default(),
                score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
            },
        }
    }
}
