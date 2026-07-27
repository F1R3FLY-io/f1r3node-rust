//! `normalize_p_collect` no longer exists as a function.
//!
//! It was three lines of glue — normalize the collection, then
//! `prepend_expr(input.par, expr, depth)` — sitting in the middle of the
//! four-frame cycle that a 577-byte program used to overflow a release node
//! with. The glue is now **fused** into the single continuation that also
//! carries `normalize_collection`'s constructor selection and `fold_match`'s
//! accumulators, so one bracket level costs one heap `NormKont` and no native
//! stack:
//!
//! * [`descend_collection`](crate::rust::interpreter::compiler::normalizer::collection_normalize_matcher::descend_collection)
//! * [`combine_collect`](crate::rust::interpreter::compiler::normalizer::collection_normalize_matcher::combine_collect)
//! * [`combine_collect_map`](crate::rust::interpreter::compiler::normalizer::collection_normalize_matcher::combine_collect_map)
//!
//! The verbatim pre-conversion `normalize_p_collect` is retained as
//! `compiler::normalize_recursive::normalize_p_collect_recursive` (test-only)
//! and is what the differential compares against.
