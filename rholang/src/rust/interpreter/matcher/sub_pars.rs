use itertools::Itertools;
use models::rhoapi::{Bundle, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use models::rust::par_bag::ParBag;

use super::par_count::ParCount;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - subPars
pub fn sub_pars(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
) -> impl Iterator<Item = (Par, Par)> {
    fn feasible_range(
        available: u64,
        minimum: u64,
        maximum: u64,
        minimum_prune: u64,
        maximum_prune: u64,
    ) -> (u64, u64) {
        let Some(after_required_prune) = available.checked_sub(minimum_prune) else {
            return (1, 0);
        };
        let upper = maximum.min(after_required_prune);
        let lower = minimum.max(available.saturating_sub(maximum_prune));
        if lower > upper {
            return (1, 0);
        }
        (lower, upper)
    }

    let (send_min, send_max) = feasible_range(
        par.family_cardinality::<Send>(),
        min.sends,
        max.sends,
        min_prune.sends,
        max_prune.sends,
    );
    let (receive_min, receive_max) = feasible_range(
        par.family_cardinality::<Receive>(),
        min.receives,
        max.receives,
        min_prune.receives,
        max_prune.receives,
    );
    let (new_min, new_max) = feasible_range(
        par.family_cardinality::<New>(),
        min.news,
        max.news,
        min_prune.news,
        max_prune.news,
    );
    let (expr_min, expr_max) = feasible_range(
        par.family_cardinality::<Expr>(),
        min.exprs,
        max.exprs,
        min_prune.exprs,
        max_prune.exprs,
    );
    let (match_min, match_max) = feasible_range(
        par.family_cardinality::<Match>(),
        min.matches,
        max.matches,
        min_prune.matches,
        max_prune.matches,
    );
    let (unf_min, unf_max) = feasible_range(
        par.family_cardinality::<GUnforgeable>(),
        min.unforgeables,
        max.unforgeables,
        min_prune.unforgeables,
        max_prune.unforgeables,
    );
    let (bundle_min, bundle_max) = feasible_range(
        par.family_cardinality::<Bundle>(),
        min.bundles,
        max.bundles,
        min_prune.bundles,
        max_prune.bundles,
    );
    let (if_min, if_max) = feasible_range(
        par.family_cardinality::<If>(),
        min.conditionals,
        max.conditionals,
        min_prune.conditionals,
        max_prune.conditionals,
    );

    let partitions = |parts: [ParBag; 8]| {
        let components = parts
            .into_iter()
            .try_fold(ParBag::new(), |components, family| components.sum(&family))
            .expect("disjoint source-family partitions cannot overflow multiplicity");
        Par::from_parts(components, Vec::new(), false)
    };

    par.components()
        .family_partitions::<Send>(send_min, send_max)
        .into_iter()
        .cartesian_product(
            par.components()
                .family_partitions::<Receive>(receive_min, receive_max),
        )
        .cartesian_product(par.components().family_partitions::<New>(new_min, new_max))
        .cartesian_product(
            par.components()
                .family_partitions::<Expr>(expr_min, expr_max),
        )
        .cartesian_product(
            par.components()
                .family_partitions::<Match>(match_min, match_max),
        )
        .cartesian_product(
            par.components()
                .family_partitions::<GUnforgeable>(unf_min, unf_max),
        )
        .cartesian_product(
            par.components()
                .family_partitions::<Bundle>(bundle_min, bundle_max),
        )
        .cartesian_product(par.components().family_partitions::<If>(if_min, if_max))
        .map(
            move |(
                (
                    (((((sub_sends, sub_receives), sub_news), sub_exprs), sub_matches), sub_unfs),
                    sub_bundles,
                ),
                sub_ifs,
            )| {
                (
                    partitions([
                        sub_sends.0,
                        sub_receives.0,
                        sub_news.0,
                        sub_exprs.0,
                        sub_matches.0,
                        sub_unfs.0,
                        sub_bundles.0,
                        sub_ifs.0,
                    ]),
                    partitions([
                        sub_sends.1,
                        sub_receives.1,
                        sub_news.1,
                        sub_exprs.1,
                        sub_matches.1,
                        sub_unfs.1,
                        sub_bundles.1,
                        sub_ifs.1,
                    ]),
                )
            },
        )
}
