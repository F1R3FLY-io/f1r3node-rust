// Observation family — fs_* handlers that READ state without
// mutating.  Mix of verifying + non-verifying per the plan taxonomy:
//
//   Non-verifying (3):
//     - `fs_flush` — fsync (slice 4.14).
//     - `fs_tell` — current fd offset.
//     - `fs_seek` — set fd offset.
//
//   Verifying (6):
//     - `fs_read`, `fs_read_at` — bounded byte read.
//     - `fs_stat` — stat record (with cmode-gated host-transient
//       field stripping).
//     - `fs_entries` — directory enumeration (two-event cost).
//     - `fs_size` — fd size.
//     - `fs_exists` — existence check.
//
// Family: [`HandlerFamily::Observation`](super::super::handler_trait::family::HandlerFamily::Observation).

pub mod fs_entries;
pub mod fs_exists;
pub mod fs_flush;
pub mod fs_read;
pub mod fs_read_at;
pub mod fs_seek;
pub mod fs_size;
pub mod fs_stat;
pub mod fs_tell;
