// Typed success/failure discriminator for a handler's reply.
//
// # Wire shape (consensus-observable)
//
// Both variants wrap a `Par` produced by the existing helpers in
// `super::super::response` (`ok_bare`, `ok_u64`, `err`, etc.), so
// the byte-level reply shape is identical to the pre-trait pattern.
// A silent drift here changes the reply bytes a Rholang caller
// pattern-matches on — a consensus-observable regression that would
// split peering.  The regression pin
// `handler_reply_err_matches_response_err_bytes` locks the invariant
// in Wave 4 before Wave 6 lights up the enforcement surface.

use models::rhoapi::Par;

use crate::rust::interpreter::io::errors::FserrCode;
use crate::rust::interpreter::io::response;

/// Typed success/failure discriminator for a handler's reply.
///
/// Both variants wrap a `Par` produced by the existing helpers in
/// [`crate::rust::interpreter::io::response`] (`ok_bare`,
/// `ok_u64`, `err`, etc.), so the byte-identical reply shape is
/// preserved across the migration — no consensus-observable
/// change.
///
/// The framework layer (yet to land, slice 4.6) will branch on the
/// variant when it needs to distinguish success from failure
/// without decoding the Par (currently only for review
/// readability; a future surface could use it for WAL Failure
/// journaling drift-checks, metrics, etc.).
pub enum HandlerReply {
    Ok(Par),
    Err(Par),
}

impl HandlerReply {
    /// Success reply — the caller supplies the already-built Par
    /// (via [`response::ok_bare`], [`response::ok_u64`], etc.).
    pub fn ok(p: Par) -> Self { HandlerReply::Ok(p) }

    /// Failure reply built via [`response::err`].  Matches the
    /// `[false, code, msg]` shape emitted by direct `response::err`
    /// call sites.
    ///
    /// `code` is typed as [`FserrCode`] so the compiler catches
    /// raw-string misuse.  All call sites must pass a canonical
    /// `FSERR_*` constant (defined in
    /// [`crate::rust::interpreter::io::errors`]) rather than an
    /// ad-hoc `&'static str`.
    pub fn err(code: FserrCode, msg: impl Into<String>) -> Self {
        HandlerReply::Err(response::err(code, msg))
    }

    /// `Box::new(HandlerReply::err(...))` in one call.  Future
    /// framework code (slice 4.5) will declare
    /// `fn parse_content(...) -> Result<Args, Box<HandlerReply>>`;
    /// the Err arm is boxed to satisfy `clippy::result_large_err`
    /// (Par is ~296 bytes; a bare `Result<T, HandlerReply>` fires
    /// the pedantic lint).  This helper spares each handler from
    /// writing `Err(Box::new(HandlerReply::err(...)))` at every
    /// bad-arg site.
    pub fn boxed_err(code: FserrCode, msg: impl Into<String>) -> Box<Self> {
        Box::new(HandlerReply::err(code, msg))
    }

    /// Consume the reply into its inner `Par`.  The framework
    /// (slice 4.6) produces `vec![reply.into_par()]` to the ack
    /// channel.
    pub fn into_par(self) -> Par {
        match self {
            HandlerReply::Ok(p) => p,
            HandlerReply::Err(p) => p,
        }
    }

    /// True for the success variant.  Reserved for downstream
    /// consumers (metrics, WAL Failure journaling).
    pub fn is_ok(&self) -> bool { matches!(self, HandlerReply::Ok(_)) }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;

    /// LOAD-BEARING: a `HandlerReply::err` call produces a Par
    /// byte-identical to the `response::err(code, msg)` call it
    /// wraps.  The handler migration's whole premise is that reply
    /// bytes stay stable across the trait boundary — a divergence
    /// here breaks consensus under Wave 6.
    #[test]
    fn handler_reply_err_matches_response_err_bytes() {
        let via_trait = HandlerReply::err(FSERR_BAD_ARG, "sample").into_par();
        let via_response = response::err(FSERR_BAD_ARG, "sample");
        assert_eq!(
            via_trait.encode_to_vec(),
            via_response.encode_to_vec(),
            "HandlerReply::err must produce a Par byte-identical to \
             response::err — the reply shape is consensus-observable \
             (WAL reply-hash verify + Rholang caller pattern-match)."
        );
    }

    /// `boxed_err` is a convenience wrapper; its unboxed Par must
    /// match the raw `err` helper.
    #[test]
    fn handler_reply_boxed_err_matches_err() {
        let boxed = HandlerReply::boxed_err(FSERR_BAD_ARG, "sample");
        let unboxed = HandlerReply::err(FSERR_BAD_ARG, "sample");
        assert_eq!(
            (*boxed).into_par().encode_to_vec(),
            unboxed.into_par().encode_to_vec()
        );
    }

    /// `HandlerReply::ok` wraps the caller's already-built Par
    /// unchanged — a drift here would mangle success replies.
    #[test]
    fn handler_reply_ok_preserves_par_bytes() {
        let built = response::ok_bare();
        let via_trait = HandlerReply::ok(built.clone()).into_par();
        assert_eq!(via_trait.encode_to_vec(), built.encode_to_vec());
    }

    /// `is_ok` discriminates variants faithfully.  Simple pin, but
    /// handlers / metrics code may condition on this surface.
    #[test]
    fn is_ok_discriminates_variants() {
        assert!(HandlerReply::ok(response::ok_bare()).is_ok());
        assert!(!HandlerReply::err(FSERR_BAD_ARG, "msg").is_ok());
    }

    /// `into_par` surfaces the wrapped Par regardless of variant —
    /// framework consumes replies via this method, so an Ok-vs-Err
    /// asymmetry here would silently drop one branch's reply bytes.
    #[test]
    fn into_par_returns_inner_for_both_variants() {
        let ok_par = response::ok_bare();
        let err_par = response::err(FSERR_BAD_ARG, "msg");

        let ok_via = HandlerReply::ok(ok_par.clone()).into_par();
        let err_via = HandlerReply::Err(err_par.clone()).into_par();

        assert_eq!(ok_via.encode_to_vec(), ok_par.encode_to_vec());
        assert_eq!(err_via.encode_to_vec(), err_par.encode_to_vec());
    }
}
