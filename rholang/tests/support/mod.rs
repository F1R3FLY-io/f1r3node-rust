//! The shared harness of the P5 integration tests of Reified RSpaces (P5 design v3, A.3.2).
//!
//! Each P5 test file includes it with `#[path = "support/mod.rs"] mod support;`. The P4 files
//! keep their inline harnesses. The harness has twelve parts:
//!
//! * [`props`]: the property runner, with the fixed seed of P5 and the case counts of P4: 64
//!   cases for a pure property or a property over the in-memory agent, and 16 cases for a
//!   property that evaluates deploys. It also holds the racing runtimes of decisions 93 and 111.
// Changed by decision 93 (decision 111): the props bullet names the racing runtimes. The old text
// follows.
// //! * [`props`]: the property runner, with the fixed seed of P5 and the case counts of P4: 64
// //!   cases for a pure property or a property over the in-memory agent, and 16 cases for a
// //!   property that evaluates deploys.
//! * [`agent`]: an in-memory agent environment over play and replay, with the operations and
//!   the readers of the agent tests.
//! * [`session`]: a native session over one runtime, as the P4 queue suite runs deploys: the
//!   setup of a space of any family, deploys that must fail with exact error codes, the native
//!   replay certification of each deploy, and the readers of records, waiters, counters and
//!   values (decision 90, C6).
// Changed by decision 90 (C6): the session runs native deploys. The legacy text follows.
// //! * [`session`]: a runtime session over a play runtime and a replay runtime, as the P4 queue
// //!   suite runs deploys: the setup of a space of any family, deploys that must fail with exact
// //!   error codes, and the readers of records, waiters, counters and values.
//! * [`observed`]: a recording accounting observer. It records each call of the accounting
//!   observer of an RSpace with its arguments (decision 90, decision 69).
//! * [`legacy`]: the host side of the host differentials. A host program runs on an RSpace whose
//!   accounting observer records its calls (decision 90, decision 69). The module was the host
//!   side of the legacy-charges differential of decision 45 (Option P), which DR-134 removed.
//! * [`native`]: the native certification of decision 47 at the v6 limits: native play, legacy
//!   play, the committed evidence, the journal check, the trace binding and native replay.
//! * [`backends`]: the test backends of the recorded VectorDB processes of P6 step 7, a mock
//!   embedder and a fake mirror, and the lock of the services of the process.
//! * [`family_programs`]: the family programs of the native suites, with a qualifier (P7).
//! * [`merge`]: the merge functions of the merge suites over a history repository: the merge
//!   index, the merged root as `dag_merger.rs` computes it, and the log filters (P9).
//! * [`coverage`]: the merge-level event coverage of a deploy: the merge of its own log over its
//!   pre-state gives its post-state (P9, amendment A4).
//! * [`process`]: the child processes of the P9 suites: the run directory, the spawn of a role,
//!   and the records of the deploys of a run (P9, amendment A13).
//! * [`blocks`]: the three blocks of the P9 suites over native space state: the 26 families with
//!   their stashed handles and names, and the reference agent (P9 steps 3 and 4).
//!
//! A test file uses only some of the helpers, so the harness allows dead code, as the shared
//! encoding strategies of P2 do (`models/tests/support/reified_strategies.rs`).

#![allow(dead_code)]

pub mod agent;
pub mod backends;
pub mod blocks;
pub mod coverage;
pub mod family_programs;
pub mod legacy;
pub mod merge;
pub mod native;
pub mod observed;
pub mod process;
pub mod props;
pub mod session;
