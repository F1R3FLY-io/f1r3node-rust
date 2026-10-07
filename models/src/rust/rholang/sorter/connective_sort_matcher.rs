// See models/src/main/scala/coop/rchain/models/rholang/sorter/ConnectiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sort_drive::sort_connective;
use super::sortable::Sortable;
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::{Connective, ConnectiveBody, Par};

pub struct ConnectiveSortMatcher;

impl ConnectiveSortMatcher {
    pub fn sort_match_metered(
        value: &Connective,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Connective>, BackingError> {
        let _depth = meter.enter()?;
        let (term, score) = match &value.connective_instance {
            Some(ConnectiveInstance::ConnAndBody(body)) => {
                let (pars, scores) = sort_body_metered(&body.ps, meter)?;
                (
                    ConnectiveInstance::ConnAndBody(ConnectiveBody { ps: pars }),
                    Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::CONNECTIVE_AND,
                        scores,
                        meter,
                    )?,
                )
            }
            Some(ConnectiveInstance::ConnOrBody(body)) => {
                let (pars, scores) = sort_body_metered(&body.ps, meter)?;
                (
                    ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: pars }),
                    Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::CONNECTIVE_OR,
                        scores,
                        meter,
                    )?,
                )
            }
            Some(ConnectiveInstance::ConnNotBody(par)) => {
                let scored = ParSortMatcher::sort_match_metered(par, meter)?;
                let mut scores = meter.vec(1)?;
                scores.push(scored.score);
                (
                    ConnectiveInstance::ConnNotBody(scored.term),
                    Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::CONNECTIVE_NOT,
                        scores,
                        meter,
                    )?,
                )
            }
            Some(ConnectiveInstance::VarRefBody(var_ref)) => (
                ConnectiveInstance::VarRefBody(meter.clone(var_ref)?),
                Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[
                        i64::from(Score::CONNECTIVE_VARREF),
                        i64::from(var_ref.index),
                        i64::from(var_ref.depth),
                    ],
                    meter,
                )?,
            ),
            Some(ConnectiveInstance::ConnBool(flag)) => flag_score(
                ConnectiveInstance::ConnBool(*flag),
                Score::CONNECTIVE_BOOL,
                *flag,
                meter,
            )?,
            Some(ConnectiveInstance::ConnInt(flag)) => flag_score(
                ConnectiveInstance::ConnInt(*flag),
                Score::CONNECTIVE_INT,
                *flag,
                meter,
            )?,
            Some(ConnectiveInstance::ConnString(flag)) => flag_score(
                ConnectiveInstance::ConnString(*flag),
                Score::CONNECTIVE_STRING,
                *flag,
                meter,
            )?,
            Some(ConnectiveInstance::ConnUri(flag)) => flag_score(
                ConnectiveInstance::ConnUri(*flag),
                Score::CONNECTIVE_URI,
                *flag,
                meter,
            )?,
            Some(ConnectiveInstance::ConnByteArray(flag)) => flag_score(
                ConnectiveInstance::ConnByteArray(*flag),
                Score::CONNECTIVE_BYTEARRAY,
                *flag,
                meter,
            )?,
            None => {
                return Ok(ScoredTerm {
                    term: Connective::default(),
                    score: Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(Score::ABSENT)),
                })
            }
        };
        Ok(ScoredTerm {
            term: Connective {
                connective_instance: Some(term),
            },
            score,
        })
    }
}

fn sort_body_metered(
    values: &[Par],
    meter: &SorterMeter<'_>,
) -> Result<(Vec<Par>, Vec<Tree<ScoreAtom>>), BackingError> {
    let mut pars = meter.vec(values.len())?;
    let mut scores = meter.vec(values.len())?;
    for value in values {
        let scored = ParSortMatcher::sort_match_metered(value, meter)?;
        pars.push(scored.term);
        scores.push(scored.score);
    }
    Ok((pars, scores))
}

fn flag_score(
    value: ConnectiveInstance,
    tag: i32,
    flag: bool,
    meter: &SorterMeter<'_>,
) -> Result<(ConnectiveInstance, Tree<ScoreAtom>), BackingError> {
    Ok((
        value,
        Tree::<ScoreAtom>::create_node_from_i64s_metered(&[i64::from(tag), flag as i64], meter)?,
    ))
}

impl Sortable<Connective> for ConnectiveSortMatcher {
    fn sort_match(c: &Connective) -> ScoredTerm<Connective> { sort_connective(c) }
}
