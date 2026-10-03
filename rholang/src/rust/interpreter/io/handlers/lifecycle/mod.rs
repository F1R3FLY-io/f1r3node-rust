// Lifecycle family — file / cap creation + retirement.
//
// Non-verifying handlers per the plan taxonomy:
//
//   - `fs_quarantine` — safe_descend_verified echo of the caller-
//     supplied joined path.  Non-verifying lifecycle helper
//     (slice 4.12).
//   - `fs_open` — allocate a FileHandle under safe_descend_verified
//     + Phase-2 real-open on Consensus caps.  `on_replay_side_effect`
//     installs a shadow FileHandle at the leader's fd (yet to land).
//   - `fs_close` — pure fd-release + Phase-2 shadow-remove on replay
//     (yet to land).
//
// Family: [`HandlerFamily::Lifecycle`](super::super::handler_trait::family::HandlerFamily::Lifecycle).

pub mod fs_quarantine;
