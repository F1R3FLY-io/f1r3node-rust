use models::rhoapi::{Par, Send};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gint_expr;
use prost::Message;

#[test]
fn capture_cost_base_par_bytes_and_sort_ties() {
    let nil = Par::default();
    let gint_one = Par::default().with_exprs(vec![new_gint_expr(1)]);
    let tied = |left: u8, right: u8| {
        let par = Par::default().with_sends(vec![
            Send {
                chan: Some(Par::default()),
                locally_free: vec![left],
                ..Send::default()
            },
            Send {
                chan: Some(Par::default()),
                locally_free: vec![right],
                ..Send::default()
            },
        ]);
        ParSortMatcher::sort_match(&par)
            .term
            .sends
            .into_iter()
            .map(|send| hex::encode(send.locally_free))
            .collect::<Vec<_>>()
    };
    println!("COST_BASE_PAR_NIL={}", hex::encode(nil.encode_to_vec()));
    println!(
        "COST_BASE_PAR_GINT_ONE={}",
        hex::encode(gint_one.encode_to_vec())
    );
    println!("COST_BASE_SORT_TIE_FORWARD={:?}", tied(1, 2));
    println!("COST_BASE_SORT_TIE_REVERSE={:?}", tied(2, 1));
}
