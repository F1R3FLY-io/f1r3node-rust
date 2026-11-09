//! A per-validator deploy queue. Clients deploy to the queue; the queue
//! holds each deploy durably and releases it to its validator at the pace
//! the shard includes and finalizes deploys, following the validator's
//! event stream rather than polling it.

pub mod engine;
pub mod error;
pub mod events;
pub mod grpc;
pub mod http;
pub mod intake;
pub mod node;
pub mod pace;
pub mod ready;
pub mod record;
pub mod store;
pub mod window;
