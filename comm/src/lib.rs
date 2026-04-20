#![allow(
    clippy::type_complexity,
    clippy::ptr_arg,
    clippy::too_many_arguments,
    clippy::module_inception,
    clippy::large_enum_variant,
    clippy::match_like_matches_macro,
    clippy::inherent_to_string,
    clippy::mixed_attributes_style,
    clippy::needless_range_loop,
    clippy::should_implement_trait,
    clippy::manual_memcpy,
    clippy::unnecessary_sort_by,
    clippy::borrowed_box,
    clippy::match_single_binding,
    clippy::unnecessary_unwrap,
    clippy::redundant_iter_cloned,
    clippy::new_ret_no_self,
    clippy::empty_line_after_doc_comments,
    clippy::assertions_on_constants,
    clippy::collapsible_match,
    clippy::no_effect,
    clippy::non_canonical_partial_ord_impl,
    clippy::cloned_ref_to_slice_refs,
    clippy::extra_unused_lifetimes,
    clippy::if_same_then_else,
    clippy::manual_strip,
    clippy::manual_try_fold,
    clippy::map_identity,
    clippy::only_used_in_recursion,
    clippy::redundant_pattern_matching,
    clippy::useless_conversion,
    clippy::while_let_loop,
    clippy::wrong_self_convention,
    clippy::arc_with_non_send_sync,
    clippy::derived_hash_with_manual_eq,
    clippy::doc_lazy_continuation,
    clippy::map_entry,
    clippy::nonminimal_bool,
    clippy::not_unsafe_ptr_arg_deref,
    clippy::unnecessary_fallible_conversions
)]

pub mod rust;

pub mod comm {
    include!(concat!(env!("OUT_DIR"), "/coop.rchain.comm.discovery.rs"));
}
