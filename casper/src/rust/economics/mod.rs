//! Per-validator earning capacity, tracked as a leaky bucket.
//!
//! Each committee member has a bucket that drains a fixed fraction of its
//! capacity on every block this node accepts, and refills when that member
//! is the one who produced the block. A member that stops producing drains
//! towards a threshold; reward earned past capacity accrues separately as
//! overflow.
//!
//! All of it is node-local. The levels depend on which blocks this node has
//! accepted, so they are this node's opinion rather than shared state.

pub mod bucket_view;
pub mod claims;
pub mod dishonest_vote;
pub mod rebuild;
pub mod reward;
