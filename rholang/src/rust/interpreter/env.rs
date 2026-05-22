// `Env<A>` moved to the rho-pure-eval crate so the rholang matcher
// (which evaluates `where`-clause guards via rho_pure_eval as of
// Phase 7) and the full process reducer can share the same type
// without a dependency cycle. Re-exported here so existing call sites
// that import from `crate::rust::interpreter::env::Env` keep working
// unchanged.

pub use rho_pure_eval::Env;
