#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::inherent_to_string)]
#![allow(clippy::clone_on_ref_ptr)]
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::cloned_ref_to_slice_refs)]

pub mod rust;

pub mod comm {
    include!(concat!(env!("OUT_DIR"), "/coop.rchain.comm.discovery.rs"));
}
