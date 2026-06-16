//! Native recognition of cost-accounted surface syntax (W1).
//!
//! Ports the transpiler's **Part A** (surface-syntax recognition + signature
//! resolution) RETARGETED to native metering: a signed term `{% P %}[s]` or token
//! stack `s :: S` resolves its signature(s) to a native
//! [`accounting::Sig`](crate::rust::interpreter::accounting::Sig) and lowers the
//! inner process ORDINARILY (the reducer meters per-COMM). The transpiler's
//! **Part B** §8 Par-gate lowering (`lower` / `signed_term::build_gates` /
//! `token` send-chains / `infra` / `oslf`) is DROPPED — emitting explicit
//! `for(t <- Σ⟦s⟧)` gates would DOUBLE-METER on the native reducer (design
//! §0/§3), and DR-13 forbids a normalizer write to `Σ⟦s⟧`.

pub mod desugar;
pub mod ir;
pub mod pattern_guard;
pub mod recognize;
pub mod sig;

#[cfg(test)]
mod tests;
