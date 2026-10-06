// Stream family — per-fd directory-entries streaming primitives
// backing `Dir.rho::entries()`.  Three non-verifying handlers in
// the family per the plan taxonomy:
//
//   - `fs_entries_stream_close` — pure stream-fd release + Phase-2
//     shadow-remove on replay (slice 4.22).
//   - `fs_entries_stream_open`  — allocate a stream fd, `openat` +
//     `fdopendir` under `safe_descend_verified`.  Consensus caps
//     rejected (readdir order not stable across per-validator
//     subdirs).  Yet to land.
//   - `fs_entries_stream_next`  — yield one entry per call via
//     `readdir_one_entry`; two-branch cost (setup + per-entry
//     supplement) via `post_reply_supplement`.  Yet to land.
//
// Family: [`HandlerFamily::Stream`](super::super::handler_trait::family::HandlerFamily::Stream).

pub mod fs_entries_stream_close;
