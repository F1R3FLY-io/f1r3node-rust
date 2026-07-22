//! File I/O native-primitive layer per FIP 2026-02-06 File-I/O.
//!
//! This module implements the *native syscall* layer described in the
//! FIP: a set of thin system processes that bridge `tokio::fs` calls
//! to Rholang messages. It knows nothing about agents, range locks,
//! or line-vs-byte mutexes -- those live in the Rholang agent classes
//! (`FsFile.rho`, `FsDir.rho`, ...) that sit on top of this layer.
//!
//! The URNs registered here are internal (namespace `rho:io:fs:native`);
//! user-facing code goes through the `Fs` agent under `rho:io:fs:1.*`
//! per the FIP.

pub mod agents;
pub mod handle_table;
pub mod injections;
pub mod mode;
#[cfg(unix)]
pub mod nss;
pub mod path;
pub mod response;
pub mod stat;
