// See models/src/main/scala/coop/rchain/models/rholang/sorter/ParSortMatcher.scala

use crate::{
    rhoapi::{Bundle, Connective, Expr, GUnforgeable, Match, New, Par, Receive, Send},
    rust::rholang::sorter::{
        bundle_sort_matcher::BundleSortMatcher,
        connective_sort_matcher::ConnectiveSortMatcher,
        expr_sort_matcher::ExprSortMatcher,
        match_sort_matcher::MatchSortMatcher,
        new_sort_matcher::NewSortMatcher,
        receive_sort_matcher::ReceiveSortMatcher,
        score_tree::{Score, ScoreAtom, Tree},
        unforgeable_sort_matcher::UnforgeableSortMatcher,
    },
};

use super::{score_tree::ScoredTerm, send_sort_matcher::SendSortMatcher, sortable::Sortable};

pub struct ParSortMatcher;

impl Sortable<Par> for ParSortMatcher {
    fn sort_match(par: &Par) -> ScoredTerm<Par> {
        let sends: Vec<ScoredTerm<Send>> = {
            let mut _sends: Vec<ScoredTerm<Send>> = par
                .sends
                .iter()
                .map(SendSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _sends);
            _sends
        };

        let receives: Vec<ScoredTerm<Receive>> = {
            let mut _receives: Vec<ScoredTerm<Receive>> = par
                .receives
                .iter()
                .map(ReceiveSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _receives);
            _receives
        };

        let exprs: Vec<ScoredTerm<Expr>> = {
            let mut _exprs: Vec<ScoredTerm<Expr>> = par
                .exprs
                .iter()
                .map(ExprSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _exprs);
            _exprs
        };

        let news: Vec<ScoredTerm<New>> = {
            let mut _news: Vec<ScoredTerm<New>> = par
                .news
                .iter()
                .map(NewSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _news);
            _news
        };

        let matches: Vec<ScoredTerm<Match>> = {
            let mut _matches: Vec<ScoredTerm<Match>> = par
                .matches
                .iter()
                .map(MatchSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _matches);
            _matches
        };

        let bundles: Vec<ScoredTerm<Bundle>> = {
            let mut _bundles: Vec<ScoredTerm<Bundle>> = par
                .bundles
                .iter()
                .map(BundleSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _bundles);
            _bundles
        };

        let connectives: Vec<ScoredTerm<Connective>> = {
            let mut _connectives: Vec<ScoredTerm<Connective>> = par
                .connectives
                .iter()
                .map(ConnectiveSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _connectives);
            _connectives
        };

        let unforgeables: Vec<ScoredTerm<GUnforgeable>> = {
            let mut _unforgeables: Vec<ScoredTerm<GUnforgeable>> = par
                .unforgeables
                .iter()
                .map(UnforgeableSortMatcher::sort_match)
                .collect();

            ScoredTerm::sort_vec(&mut _unforgeables);
            _unforgeables
        };

        let (send_terms, send_scores) = split_scored_terms(sends);
        let (receive_terms, receive_scores) = split_scored_terms(receives);
        let (news_terms, news_scores) = split_scored_terms(news);
        let (expr_terms, expr_scores) = split_scored_terms(exprs);
        let (match_terms, match_scores) = split_scored_terms(matches);
        let (bundle_terms, bundle_scores) = split_scored_terms(bundles);
        let (connective_terms, connective_scores) = split_scored_terms(connectives);
        let (unforgeable_terms, unforgeable_scores) = split_scored_terms(unforgeables);

        let sorted_par = Par {
            sends: send_terms,
            receives: receive_terms,
            news: news_terms,
            exprs: expr_terms,
            matches: match_terms,
            unforgeables: unforgeable_terms,
            bundles: bundle_terms,
            connectives: connective_terms,
            locally_free: par.locally_free.clone(),
            connective_used: par.connective_used,
        };

        let connective_used_score: i64 = if par.connective_used { 1 } else { 0 };
        let par_score = Tree::<ScoreAtom>::create_node_from_i32(
            Score::PAR,
            send_scores
                .into_iter()
                .chain(
                    receive_scores
                        .into_iter()
                        .chain(expr_scores.into_iter())
                        .chain(news_scores.into_iter())
                        .chain(match_scores.into_iter())
                        .chain(bundle_scores.into_iter())
                        .chain(connective_scores.into_iter())
                        .chain(unforgeable_scores.into_iter())
                        .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                            connective_used_score,
                        )]),
                )
                .collect(),
        );

        ScoredTerm {
            term: sorted_par,
            score: par_score,
        }
    }
}

fn split_scored_terms<T>(scored_terms: Vec<ScoredTerm<T>>) -> (Vec<T>, Vec<Tree<ScoreAtom>>) {
    let mut terms = Vec::with_capacity(scored_terms.len());
    let mut scores = Vec::with_capacity(scored_terms.len());

    for scored in scored_terms {
        terms.push(scored.term);
        scores.push(scored.score);
    }

    (terms, scores)
}
