//! MeTTaIL DDL and module system: parser and theory elaborator.
//!
//! Implements the surface frozen in
//! `docs/plans/mettail-ddl-and-modules-2026-08-19.md`, decisions D1-D10, and
//! the six additions of §3.4:
//!
//! | Gap | Where |
//! |-----|-------|
//! | G1 `Types` builder                     | [`ast::Builder::Types`] |
//! | G2 collection sorts                    | [`ast::Sort::Coll`] |
//! | G3 `...rest` remainder patterns        | [`ast::Ast::Remainder`] |
//! | G4 `^x.p` abstractions, 2-arg `subst`  | [`ast::Ast::Abs`], [`ast::Ast::Subst`] |
//! | G5 implicit `Empty` base               | `parse::Parser::te_builders` |
//! | G6 argument-reference concrete syntax  | [`ast::Item`] |

pub mod ast;
pub mod diag;
pub mod interp;
pub mod lex;
pub mod parse;
pub mod pres;
pub mod resolve;

pub use diag::{Diag, DiagKind};
pub use pres::Presentation;

/// Parse, resolve, and elaborate, starting from `entry_url`.
pub fn elaborate(entry_url: &str, r: &dyn resolve::Resolver) -> Result<Presentation, Diag> {
    let prog = resolve::Program::load(entry_url, r)?;
    let mut i = interp::Interp::new(&prog);
    i.run()
}
