//! A per-validator deploy queue. Clients deploy to the queue; the queue
//! holds each deploy durably and releases it to its validator as fast as
//! the validator takes deploys into its blocks, following the validator's
//! event stream rather than polling it.

pub mod engine;
pub mod error;
pub mod events;
pub mod grpc;
pub mod health;
pub mod http;
pub mod node;
pub mod record;
pub mod store;
