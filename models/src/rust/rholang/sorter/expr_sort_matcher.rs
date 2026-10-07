// See models/src/main/scala/coop/rchain/models/rholang/sorter/ExprSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{
    EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus, EMinusMinus, EMod, EMult, ENeg,
    ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, EVar, EZipper, Expr, Par, Var,
};
use crate::rust::par_map::ParMap;
use crate::rust::par_map_type_mapper::ParMapTypeMapper;
use crate::rust::par_set::ParSet;
use crate::rust::par_set_type_mapper::ParSetTypeMapper;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};
use crate::rust::rholang::sorter::var_sort_matcher::VarSortMatcher;
use crate::rust::sorted_par_hash_set::SortedParHashSet;

pub struct ExprSortMatcher;

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> {
        fn construct_expr(expr_instance: ExprInstance, score: Tree<ScoreAtom>) -> ScoredTerm<Expr> {
            ScoredTerm {
                term: Expr {
                    expr_instance: Some(expr_instance),
                },
                score,
            }
        }

        fn remainder_score(remainder: &Option<Var>) -> Tree<ScoreAtom> {
            match remainder {
                Some(_var) => VarSortMatcher::sort_match(_var).score,
                None => Tree::<ScoreAtom>::create_leaf_from_i64(-1),
            }
        }

        match &e.expr_instance {
            Some(expr) => match expr {
                ExprInstance::ENegBody(en) => {
                    let sorted_par = ParSortMatcher::sort_match(
                        en.p.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::ENegBody(ENeg {
                            p: Some(sorted_par.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::ENEG, vec![
                            sorted_par.score,
                        ]),
                    )
                }

                ExprInstance::EVarBody(ev) => {
                    let sorted_var = VarSortMatcher::sort_match(
                        ev.v.as_ref().expect("var field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EVarBody(EVar {
                            v: Some(sorted_var.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EVAR, vec![
                            sorted_var.score,
                        ]),
                    )
                }

                ExprInstance::ENotBody(en) => {
                    let sorted_par = ParSortMatcher::sort_match(
                        en.p.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::ENotBody(ENot {
                            p: Some(sorted_par.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::ENOT, vec![
                            sorted_par.score,
                        ]),
                    )
                }

                ExprInstance::EMultBody(em) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        em.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        em.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EMultBody(EMult {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EMULT, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EDivBody(ed) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ed.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ed.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EDivBody(EDiv {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EDIV, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EModBody(ed) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ed.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ed.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EModBody(EMod {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EMOD, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EPlusBody(ep) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ep.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ep.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EPlusBody(EPlus {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EPLUS, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EMinusBody(em) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        em.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        em.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EMinusBody(EMinus {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EMINUS, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::ELtBody(el) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        el.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        el.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::ELtBody(ELt {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::ELT, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::ELteBody(el) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        el.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        el.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::ELteBody(ELte {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::ELTE, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EGtBody(eg) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        eg.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        eg.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EGtBody(EGt {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EGT, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EGteBody(eg) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        eg.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        eg.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EGteBody(EGte {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EGTE, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EEqBody(ee) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ee.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ee.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EEqBody(EEq {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EEQ, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::ENeqBody(en) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        en.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        en.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::ENeqBody(ENeq {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::ENEQ, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EAndBody(ea) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ea.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ea.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EAndBody(EAnd {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EAND, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EOrBody(eo) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        eo.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        eo.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EOrBody(EOr {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EOR, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EMatchesBody(em) => {
                    let sorted_target = ParSortMatcher::sort_match(
                        em.target
                            .as_ref()
                            .expect("target field was None, should be Some"),
                    );
                    let sorted_pattern = ParSortMatcher::sort_match(
                        em.pattern
                            .as_ref()
                            .expect("pattern field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EMatchesBody(EMatches {
                            target: Some(sorted_target.term),
                            pattern: Some(sorted_pattern.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EMATCHES, vec![
                            sorted_target.score,
                            sorted_pattern.score,
                        ]),
                    )
                }

                ExprInstance::EPercentPercentBody(ep) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ep.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ep.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EPercentPercentBody(EPercentPercent {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EPERCENT, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EPlusPlusBody(ep) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        ep.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        ep.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EPlusPlusBody(EPlusPlus {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EPLUSPLUS, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EMinusMinusBody(em) => {
                    let sorted_par1 = ParSortMatcher::sort_match(
                        em.p1.as_ref().expect("par field was None, should be Some"),
                    );
                    let sorted_par2 = ParSortMatcher::sort_match(
                        em.p2.as_ref().expect("par field was None, should be Some"),
                    );

                    construct_expr(
                        ExprInstance::EMinusMinusBody(EMinusMinus {
                            p1: Some(sorted_par1.term),
                            p2: Some(sorted_par2.term),
                        }),
                        Tree::<ScoreAtom>::create_node_from_i32(Score::EMINUSMINUS, vec![
                            sorted_par1.score,
                            sorted_par2.score,
                        ]),
                    )
                }

                ExprInstance::EMapBody(emap) => {
                    let par_map = ParMapTypeMapper::emap_to_par_map(emap.clone());

                    fn sort_key_value_pair(key: &Par, value: &Par) -> ScoredTerm<(Par, Par)> {
                        let sorted_key = ParSortMatcher::sort_match(key);
                        let sorted_value = ParSortMatcher::sort_match(value);

                        ScoredTerm {
                            term: (sorted_key.term, sorted_value.term),
                            score: sorted_key.score,
                        }
                    }

                    let sorted_pars: Vec<ScoredTerm<(Par, Par)>> = par_map
                        .ps
                        .sorted_list
                        .iter()
                        .map(|kv| sort_key_value_pair(&kv.0, &kv.1))
                        .collect();

                    let remainder_score = remainder_score(&par_map.remainder);
                    let connective_used_score: i64 = if par_map.connective_used { 1 } else { 0 };

                    construct_expr(
                        ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(ParMap::new(
                            sorted_pars.clone().into_iter().map(|p| p.term).collect(),
                            par_map.connective_used,
                            par_map.locally_free,
                            par_map.remainder,
                        ))),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EMAP as i64),
                                remainder_score,
                            ]
                            .into_iter()
                            .chain(sorted_pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::ESetBody(eset) => {
                    let par_set = ParSetTypeMapper::eset_to_par_set(eset.clone());
                    let sorted_pars: Vec<ScoredTerm<Par>> = par_set
                        .ps
                        .sorted_pars
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let remainder_score = remainder_score(&par_set.remainder);
                    let connective_used_score: i64 = if par_set.connective_used { 1 } else { 0 };

                    construct_expr(
                        ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet {
                            ps: SortedParHashSet::create_from_vec(
                                sorted_pars.clone().into_iter().map(|p| p.term).collect(),
                            ),
                            connective_used: par_set.connective_used,
                            locally_free: par_set.locally_free,
                            remainder: par_set.remainder,
                        })),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::ESET as i64),
                                remainder_score,
                            ]
                            .into_iter()
                            .chain(sorted_pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::EPathmapBody(pathmap) => {
                    // Similar to EListBody - sort all Par elements in the pathmap
                    let pars: Vec<ScoredTerm<Par>> = pathmap
                        .ps
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let remainder_score = remainder_score(&pathmap.remainder);
                    let connective_used_score: i64 = if pathmap.connective_used { 1 } else { 0 };

                    construct_expr(
                        ExprInstance::EPathmapBody(EPathMap {
                            ps: pars.clone().into_iter().map(|p| p.term).collect(),
                            locally_free: pathmap.locally_free.clone(),
                            connective_used: pathmap.connective_used,
                            remainder: pathmap.remainder.clone(),
                        }),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EPATHMAP as i64),
                                remainder_score,
                            ]
                            .into_iter()
                            .chain(pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::EListBody(list) => {
                    let pars: Vec<ScoredTerm<Par>> = list
                        .ps
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let remainder_score = remainder_score(&list.remainder);
                    let connective_used_score: i64 = if list.connective_used { 1 } else { 0 };

                    construct_expr(
                        ExprInstance::EListBody(EList {
                            ps: pars.clone().into_iter().map(|p| p.term).collect(),
                            locally_free: list.locally_free.clone(),
                            connective_used: list.connective_used,
                            remainder: list.remainder.clone(),
                        }),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::ELIST as i64),
                                remainder_score,
                            ]
                            .into_iter()
                            .chain(pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::EZipperBody(zipper) => {
                    // Sort the zipper's PathMap and maintain zipper metadata
                    let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                    let pars: Vec<ScoredTerm<Par>> = pathmap
                        .ps
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let connective_used_score: i64 = if zipper.connective_used { 1 } else { 0 };

                    construct_expr(
                        ExprInstance::EZipperBody(EZipper {
                            pathmap: Some(EPathMap {
                                ps: pars.clone().into_iter().map(|p| p.term).collect(),
                                locally_free: pathmap.locally_free.clone(),
                                connective_used: pathmap.connective_used,
                                remainder: pathmap.remainder.clone(),
                            }),
                            current_path: zipper.current_path.clone(),
                            is_write_zipper: zipper.is_write_zipper,
                            locally_free: zipper.locally_free.clone(),
                            connective_used: zipper.connective_used,
                        }),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EPATHMAP as i64 + 1), // Use EPATHMAP + 1 for zipper
                            ]
                            .into_iter()
                            .chain(pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::ETupleBody(tuple) => {
                    let sorted_pars: Vec<ScoredTerm<Par>> = tuple
                        .ps
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let connective_used_score: i64 = if tuple.connective_used { 1 } else { 0 };
                    let mut tuple_cloned = tuple.clone();
                    tuple_cloned.ps = sorted_pars.clone().into_iter().map(|p| p.term).collect();

                    construct_expr(
                        ExprInstance::ETupleBody(tuple_cloned),
                        Tree::Node(
                            vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                Score::ETUPLE as i64,
                            )]
                            .into_iter()
                            .chain(sorted_pars.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::EMethodBody(em) => {
                    let args: Vec<ScoredTerm<Par>> = em
                        .arguments
                        .iter()
                        .map(|p| ParSortMatcher::sort_match(p))
                        .collect();

                    let sorted_target = ParSortMatcher::sort_match(
                        em.target
                            .as_ref()
                            .expect("target field on EMethod was None, should be Some"),
                    );
                    let connective_used_score: i64 = if em.connective_used { 1 } else { 0 };

                    let mut em_cloned = em.clone();
                    em_cloned.arguments = args.clone().into_iter().map(|p| p.term).collect();
                    em_cloned.target = Some(sorted_target.term);

                    construct_expr(
                        ExprInstance::EMethodBody(em_cloned),
                        Tree::Node(
                            vec![
                                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EMETHOD as i64),
                                Tree::<ScoreAtom>::create_leaf_from_string(em.method_name.clone()),
                                sorted_target.score,
                            ]
                            .into_iter()
                            .chain(args.into_iter().map(|p| p.score))
                            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                                connective_used_score,
                            )])
                            .collect(),
                        ),
                    )
                }

                ExprInstance::GBool(gb) => {
                    // See models/src/main/scala/coop/rchain/models/rholang/sorter/BoolSortMatcher.scala
                    let sorted = if *gb {
                        ScoredTerm {
                            term: gb,
                            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                                Score::BOOL as i64,
                                0,
                            ]),
                        }
                    } else {
                        ScoredTerm {
                            term: gb,
                            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                                Score::BOOL as i64,
                                1,
                            ]),
                        }
                    };

                    ScoredTerm {
                        term: e.clone(),
                        score: sorted.score,
                    }
                }

                ExprInstance::GInt(gi) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::INT as i64, *gi]),
                },

                ExprInstance::GString(gs) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::STRING, vec![
                        Tree::<ScoreAtom>::create_leaf_from_string(gs.clone()),
                    ]),
                },

                ExprInstance::GUri(gu) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::URI, vec![
                        Tree::<ScoreAtom>::create_leaf_from_string(gu.clone()),
                    ]),
                },

                ExprInstance::GByteArray(ba) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::EBYTEARR, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(ba.clone()),
                    ]),
                },

                ExprInstance::GDouble(bits) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::DOUBLE as i64,
                        *bits as i64,
                    ]),
                },

                ExprInstance::GFloat32(bits) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::FLOAT32 as i64,
                        *bits as i64,
                    ]),
                },

                ExprInstance::GBigInt(bytes) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::BIG_INT, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(bytes.clone()),
                    ]),
                },

                ExprInstance::GBigRat(rat) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::BIG_RAT, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(rat.numerator.clone()),
                        Tree::<ScoreAtom>::create_leaf_from_bytes(rat.denominator.clone()),
                    ]),
                },

                ExprInstance::GFixedPoint(fp) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::FIXED_POINT, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(fp.unscaled.clone()),
                        Tree::<ScoreAtom>::create_node_from_i64s(vec![fp.scale as i64]),
                    ]),
                },

                ExprInstance::GUint64(u) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::UINT64, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(u.to_be_bytes().to_vec()),
                    ]),
                },

                ExprInstance::GInt32(x) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::INT32 as i64,
                        *x as i64,
                    ]),
                },

                ExprInstance::GUint32(x) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::UINT32 as i64,
                        *x as i64,
                    ]),
                },

                ExprInstance::GUint16(x) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::UINT16 as i64,
                        *x as i64,
                    ]),
                },

                ExprInstance::GUint8(x) => ScoredTerm {
                    term: e.clone(),
                    score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                        Score::UINT8 as i64,
                        *x as i64,
                    ]),
                },
            },

            // TODO get rid of Empty nodes in Protobuf unless they represent sth indeed optional - OLD
            None => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::ABSENT, Vec::new()),
            },
        }
    }
}

fn construct_metered(expr_instance: ExprInstance, score: Tree<ScoreAtom>) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: Expr {
            expr_instance: Some(expr_instance),
        },
        score,
    }
}

fn score_children(
    first: Tree<ScoreAtom>,
    second: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Vec<Tree<ScoreAtom>>, BackingError> {
    let mut scores = meter.vec(2)?;
    scores.push(first);
    scores.push(second);
    Ok(scores)
}

fn score_node(
    kind: i32,
    score: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let mut children = meter.vec(1)?;
    children.push(score);
    Tree::<ScoreAtom>::create_node_from_i32_metered(kind, children, meter)
}

fn sort_pars_metered(
    pars: &[Par],
    meter: &SorterMeter<'_>,
) -> Result<Vec<ScoredTerm<Par>>, BackingError> {
    let mut result = meter.vec(pars.len())?;
    for par in pars {
        result.push(ParSortMatcher::sort_match_metered(par, meter)?);
    }
    Ok(result)
}

fn split_pars_metered(
    pars: Vec<ScoredTerm<Par>>,
    meter: &SorterMeter<'_>,
) -> Result<(Vec<Par>, Vec<Tree<ScoreAtom>>), BackingError> {
    let mut terms = meter.vec(pars.len())?;
    let mut scores = meter.vec(pars.len())?;
    for par in pars {
        terms.push(par.term);
        scores.push(par.score);
    }
    Ok((terms, scores))
}

fn collection_score(
    kind: i32,
    remainder: &Option<Var>,
    elements: Vec<Tree<ScoreAtom>>,
    connective_used: bool,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let count = elements
        .len()
        .checked_add(3)
        .ok_or(BackingError::Overflow)?;
    let mut scores = meter.vec(count)?;
    scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(kind as i64));
    scores.push(match remainder {
        Some(var) => VarSortMatcher::sort_match_metered(var, meter)?.score,
        None => Tree::<ScoreAtom>::create_leaf_from_i64(-1),
    });
    scores.extend(elements);
    scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
        connective_used,
    )));
    Ok(Tree::Node(scores))
}

macro_rules! sort_binary_expr_metered {
    ($value:expr, $variant:ident, $ty:ident, $score:expr, $meter:expr) => {{
        let left = ParSortMatcher::sort_match_metered(
            $value.p1.as_ref().ok_or(BackingError::Rejected)?,
            $meter,
        )?;
        let right = ParSortMatcher::sort_match_metered(
            $value.p2.as_ref().ok_or(BackingError::Rejected)?,
            $meter,
        )?;
        Ok(construct_metered(
            ExprInstance::$variant($ty {
                p1: Some(left.term),
                p2: Some(right.term),
            }),
            Tree::<ScoreAtom>::create_node_from_i32_metered(
                $score,
                score_children(left.score, right.score, $meter)?,
                $meter,
            )?,
        ))
    }};
}

impl ExprSortMatcher {
    pub fn sort_match_metered(
        expr: &Expr,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let _depth = meter.enter()?;
        meter.reserve(1, std::mem::size_of::<Expr>(), 0)?;
        match &expr.expr_instance {
            Some(ExprInstance::ENegBody(value)) => {
                let sorted = ParSortMatcher::sort_match_metered(
                    value.p.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::ENegBody(ENeg {
                        p: Some(sorted.term),
                    }),
                    score_node(Score::ENEG, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::ENotBody(value)) => {
                let sorted = ParSortMatcher::sort_match_metered(
                    value.p.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::ENotBody(ENot {
                        p: Some(sorted.term),
                    }),
                    score_node(Score::ENOT, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::EVarBody(value)) => {
                let sorted = VarSortMatcher::sort_match_metered(
                    value.v.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EVarBody(EVar {
                        v: Some(sorted.term),
                    }),
                    score_node(Score::EVAR, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::EMultBody(value)) => {
                sort_binary_expr_metered!(value, EMultBody, EMult, Score::EMULT, meter)
            }
            Some(ExprInstance::EDivBody(value)) => {
                sort_binary_expr_metered!(value, EDivBody, EDiv, Score::EDIV, meter)
            }
            Some(ExprInstance::EModBody(value)) => {
                sort_binary_expr_metered!(value, EModBody, EMod, Score::EMOD, meter)
            }
            Some(ExprInstance::EPlusBody(value)) => {
                sort_binary_expr_metered!(value, EPlusBody, EPlus, Score::EPLUS, meter)
            }
            Some(ExprInstance::EMinusBody(value)) => {
                sort_binary_expr_metered!(value, EMinusBody, EMinus, Score::EMINUS, meter)
            }
            Some(ExprInstance::ELtBody(value)) => {
                sort_binary_expr_metered!(value, ELtBody, ELt, Score::ELT, meter)
            }
            Some(ExprInstance::ELteBody(value)) => {
                sort_binary_expr_metered!(value, ELteBody, ELte, Score::ELTE, meter)
            }
            Some(ExprInstance::EGtBody(value)) => {
                sort_binary_expr_metered!(value, EGtBody, EGt, Score::EGT, meter)
            }
            Some(ExprInstance::EGteBody(value)) => {
                sort_binary_expr_metered!(value, EGteBody, EGte, Score::EGTE, meter)
            }
            Some(ExprInstance::EEqBody(value)) => {
                sort_binary_expr_metered!(value, EEqBody, EEq, Score::EEQ, meter)
            }
            Some(ExprInstance::ENeqBody(value)) => {
                sort_binary_expr_metered!(value, ENeqBody, ENeq, Score::ENEQ, meter)
            }
            Some(ExprInstance::EAndBody(value)) => {
                sort_binary_expr_metered!(value, EAndBody, EAnd, Score::EAND, meter)
            }
            Some(ExprInstance::EOrBody(value)) => {
                sort_binary_expr_metered!(value, EOrBody, EOr, Score::EOR, meter)
            }
            Some(ExprInstance::EPercentPercentBody(value)) => {
                sort_binary_expr_metered!(
                    value,
                    EPercentPercentBody,
                    EPercentPercent,
                    Score::EPERCENT,
                    meter
                )
            }
            Some(ExprInstance::EPlusPlusBody(value)) => {
                sort_binary_expr_metered!(value, EPlusPlusBody, EPlusPlus, Score::EPLUSPLUS, meter)
            }
            Some(ExprInstance::EMinusMinusBody(value)) => {
                sort_binary_expr_metered!(
                    value,
                    EMinusMinusBody,
                    EMinusMinus,
                    Score::EMINUSMINUS,
                    meter
                )
            }
            Some(ExprInstance::EMatchesBody(value)) => {
                let target = ParSortMatcher::sort_match_metered(
                    value.target.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                let pattern = ParSortMatcher::sort_match_metered(
                    value.pattern.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EMatchesBody(EMatches {
                        target: Some(target.term),
                        pattern: Some(pattern.term),
                    }),
                    Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::EMATCHES,
                        score_children(target.score, pattern.score, meter)?,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EListBody(list)) => {
                let (ps, scores) = split_pars_metered(sort_pars_metered(&list.ps, meter)?, meter)?;
                Ok(construct_metered(
                    ExprInstance::EListBody(EList {
                        ps,
                        locally_free: meter.clone(&list.locally_free)?,
                        connective_used: list.connective_used,
                        remainder: meter.clone(&list.remainder)?,
                    }),
                    collection_score(
                        Score::ELIST,
                        &list.remainder,
                        scores,
                        list.connective_used,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EPathmapBody(pathmap)) => {
                let (ps, scores) =
                    split_pars_metered(sort_pars_metered(&pathmap.ps, meter)?, meter)?;
                Ok(construct_metered(
                    ExprInstance::EPathmapBody(EPathMap {
                        ps,
                        locally_free: meter.clone(&pathmap.locally_free)?,
                        connective_used: pathmap.connective_used,
                        remainder: meter.clone(&pathmap.remainder)?,
                    }),
                    collection_score(
                        Score::EPATHMAP,
                        &pathmap.remainder,
                        scores,
                        pathmap.connective_used,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EZipperBody(zipper)) => {
                let pathmap = zipper.pathmap.as_ref().ok_or(BackingError::Rejected)?;
                let (ps, scores) =
                    split_pars_metered(sort_pars_metered(&pathmap.ps, meter)?, meter)?;
                let score = simple_collection_score(
                    Score::EPATHMAP + 1,
                    scores,
                    zipper.connective_used,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EZipperBody(EZipper {
                        pathmap: Some(EPathMap {
                            ps,
                            locally_free: meter.clone(&pathmap.locally_free)?,
                            connective_used: pathmap.connective_used,
                            remainder: meter.clone(&pathmap.remainder)?,
                        }),
                        current_path: meter.clone(&zipper.current_path)?,
                        is_write_zipper: zipper.is_write_zipper,
                        locally_free: meter.clone(&zipper.locally_free)?,
                        connective_used: zipper.connective_used,
                    }),
                    score,
                ))
            }
            Some(ExprInstance::ETupleBody(tuple)) => {
                let (ps, scores) = split_pars_metered(sort_pars_metered(&tuple.ps, meter)?, meter)?;
                let mut sorted_tuple = meter.clone(tuple)?;
                sorted_tuple.ps = ps;
                Ok(construct_metered(
                    ExprInstance::ETupleBody(sorted_tuple),
                    simple_collection_score(Score::ETUPLE, scores, tuple.connective_used, meter)?,
                ))
            }
            Some(ExprInstance::EMethodBody(method)) => {
                let (arguments, scores) =
                    split_pars_metered(sort_pars_metered(&method.arguments, meter)?, meter)?;
                let target = ParSortMatcher::sort_match_metered(
                    method.target.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                let count = scores.len().checked_add(4).ok_or(BackingError::Overflow)?;
                let mut score_items = meter.vec(count)?;
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_i64(
                    Score::EMETHOD as i64,
                ));
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_string(
                    meter.clone(&method.method_name)?,
                ));
                score_items.push(target.score);
                score_items.extend(scores);
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                    method.connective_used,
                )));
                let mut sorted_method = meter.clone(method)?;
                sorted_method.arguments = arguments;
                sorted_method.target = Some(target.term);
                Ok(construct_metered(
                    ExprInstance::EMethodBody(sorted_method),
                    Tree::Node(score_items),
                ))
            }
            Some(ExprInstance::ESetBody(set)) => Self::sort_set_metered(set, meter),
            Some(ExprInstance::EMapBody(map)) => Self::sort_map_metered(map, meter),
            Some(ExprInstance::GBool(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::BOOL as i64, i64::from(!*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GInt(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::INT as i64, *value],
                    meter,
                )?,
            }),
            Some(ExprInstance::GString(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::STRING,
                    Tree::<ScoreAtom>::create_leaf_from_string(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GUri(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::URI,
                    Tree::<ScoreAtom>::create_leaf_from_string(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GByteArray(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::EBYTEARR,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GDouble(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::DOUBLE as i64, *value as i64],
                    meter,
                )?,
            }),
            Some(ExprInstance::GFloat32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::FLOAT32 as i64, *value as i64],
                    meter,
                )?,
            }),
            Some(ExprInstance::GBigInt(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::BIG_INT,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GBigRat(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::BIG_RAT,
                    score_children(
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.numerator)?),
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.denominator)?),
                        meter,
                    )?,
                    meter,
                )?,
            }),
            Some(ExprInstance::GFixedPoint(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::FIXED_POINT,
                    score_children(
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.unscaled)?),
                        Tree::<ScoreAtom>::create_node_from_i64s_metered(
                            &[value.scale as i64],
                            meter,
                        )?,
                        meter,
                    )?,
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint64(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::UINT64,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(
                        meter.clone_slice(&value.to_be_bytes())?,
                    ),
                    meter,
                )?,
            }),
            Some(ExprInstance::GInt32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::INT32 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT32 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint16(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT16 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint8(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT8 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            None => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::ABSENT,
                    Vec::new(),
                    meter,
                )?,
            }),
        }
    }

    fn sort_set_metered(
        set: &crate::rhoapi::ESet,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let par_set = ParSetTypeMapper::eset_to_par_set_metered(meter.clone(set)?, meter)?;
        let (terms, scores) =
            split_pars_metered(sort_pars_metered(&par_set.ps.sorted_pars, meter)?, meter)?;
        let score = collection_score(
            Score::ESET,
            &par_set.remainder,
            scores,
            par_set.connective_used,
            meter,
        )?;
        let sorted = SortedParHashSet::create_from_vec_metered(terms, meter)?;
        Ok(construct_metered(
            ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset_prepaid(ParSet {
                ps: sorted,
                connective_used: par_set.connective_used,
                locally_free: par_set.locally_free,
                remainder: par_set.remainder,
            })),
            score,
        ))
    }

    fn sort_map_metered(
        map: &crate::rhoapi::EMap,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let par_map = ParMapTypeMapper::emap_to_par_map_metered(meter.clone(map)?, meter)?;
        let mut terms = meter.vec(par_map.ps.sorted_list.len())?;
        let mut scores = meter.vec(par_map.ps.sorted_list.len())?;
        for (key, value) in &par_map.ps.sorted_list {
            let key = ParSortMatcher::sort_match_metered(key, meter)?;
            let value = ParSortMatcher::sort_match_metered(value, meter)?;
            terms.push((key.term, value.term));
            scores.push(key.score);
        }
        let score = collection_score(
            Score::EMAP,
            &par_map.remainder,
            scores,
            par_map.connective_used,
            meter,
        )?;
        let sorted = ParMap::new_metered(
            terms,
            par_map.connective_used,
            par_map.locally_free,
            par_map.remainder,
            meter,
        )?;
        Ok(construct_metered(
            ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap_metered(sorted, meter)?),
            score,
        ))
    }
}

fn simple_collection_score(
    kind: i32,
    scores: Vec<Tree<ScoreAtom>>,
    connective_used: bool,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let count = scores.len().checked_add(2).ok_or(BackingError::Overflow)?;
    let mut items = meter.vec(count)?;
    items.push(Tree::<ScoreAtom>::create_leaf_from_i64(kind as i64));
    items.extend(scores);
    items.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
        connective_used,
    )));
    Ok(Tree::Node(items))
}

#[cfg(test)]
mod metered_tests {
    use super::*;
    use crate::rhoapi::{EMap, ESet, KeyValuePair};
    use crate::rust::utils::{new_gint_par, new_gstring_par};

    #[test]
    fn metered_expr_sort_preserves_recursive_and_collection_scores() {
        let left = new_gstring_par("left".to_owned(), Vec::new(), false);
        let right = new_gint_par(7, Vec::new(), false);
        let values = [
            Expr {
                expr_instance: Some(ExprInstance::GFloat32(1.5f32.to_bits())),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                    p1: Some(left.clone()),
                    p2: Some(right.clone()),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![left.clone(), right.clone()],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![left.clone(), left.clone(), right.clone()],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![
                        KeyValuePair {
                            key: Some(left.clone()),
                            value: Some(right.clone()),
                        },
                        KeyValuePair {
                            key: Some(left.clone()),
                            value: Some(left.clone()),
                        },
                    ],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
        ];
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for value in values {
            assert_eq!(
                ExprSortMatcher::sort_match_metered(&value, &meter).unwrap(),
                ExprSortMatcher::sort_match(&value)
            );
        }
    }

    #[test]
    fn metered_expr_sort_matches_sized_integer_scores() {
        let values = [0, 1, u64::MAX]
            .map(ExprInstance::GUint64)
            .into_iter()
            .chain([i32::MIN, -1, 0, i32::MAX].map(ExprInstance::GInt32))
            .chain([0, 1, u32::MAX].map(ExprInstance::GUint32))
            .chain([0, u32::from(u16::MAX)].map(ExprInstance::GUint16))
            .chain([0, u32::from(u8::MAX)].map(ExprInstance::GUint8));
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for expr_instance in values {
            let value = Expr {
                expr_instance: Some(expr_instance),
            };
            assert_eq!(
                ExprSortMatcher::sort_match_metered(&value, &meter)
                    .expect("a sized integer sorts within an unbounded meter"),
                ExprSortMatcher::sort_match(&value)
            );
        }
    }

    #[test]
    fn metered_expr_sort_rejects_large_ground_before_copy() {
        let value = Expr {
            expr_instance: Some(ExprInstance::GString("payload".repeat(1024))),
        };
        let reserve = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        let meter = SorterMeter::new(&reserve);
        assert!(matches!(
            ExprSortMatcher::sort_match_metered(&value, &meter),
            Err(BackingError::Rejected)
        ));
    }

    #[test]
    fn malformed_nested_expr_fields_reject_without_panicking() {
        let values = [
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                    p1: None,
                    p2: Some(Par::default()),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ENegBody(ENeg { p: None })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                    target: Some(Par::default()),
                    pattern: None,
                })),
            },
        ];
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for value in values {
            assert!(matches!(
                ExprSortMatcher::sort_match_metered(&value, &meter),
                Err(BackingError::Rejected)
            ));
        }
    }
}
