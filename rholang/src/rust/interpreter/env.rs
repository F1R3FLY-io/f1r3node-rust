// See rholang/src/main/scala/coop/rchain/rholang/interpreter/Env.scala
//
// Env<A> moved to the rho-pure-eval crate so that rspace++ (which
// evaluates guards inside the matcher in Phase 6) and rholang (which
// evaluates processes) can share the same type without a dependency
// cycle. Re-exported here so existing call sites that import from
// `crate::rust::interpreter::env::Env` keep working unchanged.

pub use rho_pure_eval::Env;
