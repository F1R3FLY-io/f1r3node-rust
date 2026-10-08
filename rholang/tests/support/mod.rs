//! The shared harness of the P5 integration tests of Reified RSpaces (P5 design v3, A.3.2).
//!
//! Each P5 test file includes it with `#[path = "support/mod.rs"] mod support;`. The P4 files
//! keep their inline harnesses. The harness has five parts:
//!
//! * [`props`]: the property runner, with the fixed seed of P5 and the case counts of P4: 64
//!   cases for a pure property or a property over the in-memory agent, and 16 cases for a
//!   property that evaluates deploys.
//! * [`agent`]: an in-memory agent environment over play and replay, with the operations and
//!   the readers of the agent tests.
//! * [`session`]: a runtime session over a play runtime and a replay runtime, as the P4 queue
//!   suite runs deploys: the setup of a space of any family, deploys that must fail with exact
//!   error codes, and the readers of records, waiters, counters and values.
//! * [`legacy`]: the host side of the legacy-charges differential of decision 45 (Option P),
//!   moved here from `reified_legacy_charges.rs`.
//! * [`native`]: the native certification of decision 47 at the v6 limits: native play, legacy
//!   play, the committed evidence, the journal check, the trace binding and native replay.
//!
//! A test file uses only some of the helpers, so the harness allows dead code, as the shared
//! encoding strategies of P2 do (`models/tests/support/reified_strategies.rs`).

#![allow(dead_code)]

pub mod agent;
pub mod legacy;
pub mod native;
pub mod props;
pub mod session;
