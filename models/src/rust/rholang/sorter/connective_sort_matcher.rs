// See models/src/main/scala/coop/rchain/models/rholang/sorter/ConnectiveSortMatcher.scala

use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::{Connective, Par};

pub struct ConnectiveSortMatcher;

impl Sortable<Connective> for ConnectiveSortMatcher {
    fn sort_match(c: &Connective) -> ScoredTerm<Connective> {
        match &c.connective_instance {
            Some(ConnectiveInstance::ConnAndBody(cb)) => {
                let pars: Vec<ScoredTerm<Par>> =
                    cb.ps.iter().map(ParSortMatcher::sort_match).collect();

                ScoredTerm {
                    term: Connective {
                        connective_instance: Some(ConnectiveInstance::ConnAndBody({
                            let mut cb_cloned = cb.clone();
                            cb_cloned.ps = pars.clone().into_iter().map(|p| p.term).collect();
                            cb_cloned
                        })),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(
                        Score::CONNECTIVE_AND,
                        pars.into_iter().map(|p| p.score).collect(),
                    ),
                }
            }

            Some(ConnectiveInstance::ConnOrBody(cb)) => {
                let pars: Vec<ScoredTerm<Par>> =
                    cb.ps.iter().map(ParSortMatcher::sort_match).collect();

                ScoredTerm {
                    term: Connective {
                        connective_instance: Some(ConnectiveInstance::ConnOrBody({
                            let mut cb_cloned = cb.clone();
                            cb_cloned.ps = pars.clone().into_iter().map(|p| p.term).collect();
                            cb_cloned
                        })),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(
                        Score::CONNECTIVE_OR,
                        pars.into_iter().map(|p| p.score).collect(),
                    ),
                }
            }

            Some(ConnectiveInstance::ConnNotBody(p)) => {
                let scored_par = ParSortMatcher::sort_match(p);
                ScoredTerm {
                    term: Connective {
                        connective_instance: Some(ConnectiveInstance::ConnNotBody(scored_par.term)),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::CONNECTIVE_NOT, vec![
                        scored_par.score,
                    ]),
                }
            }

            Some(ConnectiveInstance::VarRefBody(v)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::VarRefBody(*v)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_VARREF as i64,
                    v.index as i64,
                    v.depth as i64,
                ]),
            },

            Some(ConnectiveInstance::ConnBool(b)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnBool(*b)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_BOOL as i64,
                    {
                        if *b {
                            1
                        } else {
                            0
                        }
                    },
                ]),
            },

            Some(ConnectiveInstance::ConnInt(b)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnInt(*b)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_INT as i64,
                    {
                        if *b {
                            1
                        } else {
                            0
                        }
                    },
                ]),
            },

            Some(ConnectiveInstance::ConnString(b)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnString(*b)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_STRING as i64,
                    {
                        if *b {
                            1
                        } else {
                            0
                        }
                    },
                ]),
            },

            Some(ConnectiveInstance::ConnUri(b)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnUri(*b)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_URI as i64,
                    {
                        if *b {
                            1
                        } else {
                            0
                        }
                    },
                ]),
            },

            Some(ConnectiveInstance::ConnByteArray(b)) => ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnByteArray(*b)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::CONNECTIVE_BYTEARRAY as i64,
                    {
                        if *b {
                            1
                        } else {
                            0
                        }
                    },
                ]),
            },

            None => ScoredTerm {
                term: Connective::default(),
                score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
            },
        }
    }
}
