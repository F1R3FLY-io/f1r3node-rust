// Free helper that builds the `FSERR_CONSENSUS_DIVERGENCE` reply
// a verifying handler produces when its fresh syscall reply hash
// disagrees with the leader's cached reply hash on
// `is_replay = true` under Consensus cmode.
//
// # Why a wrapper
//
// Pre-trait, the 12 verifying-handler divergence sites all built
// this same reply inline with
// `response::err(FSERR_CONSENSUS_DIVERGENCE, format!("{handler}
// follower re-execute diverges from leader: {reason}"))`.
// The wrapper is a single source of truth for:
//
//   1. The exact FSERR constant (`FSERR_CONSENSUS_DIVERGENCE`,
//      code 13 — see `io/errors.rs`).
//   2. The exact message shape — a monitoring layer greps the
//      "follower re-execute diverges from leader" substring; a
//      silent drift would break alerting AND consensus (the reply
//      bytes are what the Rholang caller pattern-matches on).
//
// # Wire-shape pin (consensus-observable)
//
// The yet-to-land dispatcher (slice 4.8 / 4.9) will call this
// helper at the `JournalPath::VerifyDivergence` branch and produce
// the returned `Par` to the ack channel.  Under Wave 6 this is the
// consensus-observable reply bytes a verifying follower sends when
// its re-execute diverges.  A regression that reshaped the reply
// would split peering and silently break the leader / follower
// agreement on which deploys fail.
//
// Pinned by `consensus_divergence_reply_matches_pre_trait_format`:
// the wrapper output is byte-identical to the pre-trait inline
// pattern across a sample of handler names + reasons.

use models::rhoapi::Par;

use crate::rust::interpreter::io::errors::FSERR_CONSENSUS_DIVERGENCE;
use crate::rust::interpreter::io::response;

/// Build the `FSERR_CONSENSUS_DIVERGENCE` reply a verifying handler
/// produces on the `JournalPath::VerifyDivergence` branch (fresh
/// reply hash disagrees with leader's cached reply hash on
/// Consensus-cmode replay).
///
/// `handler_name` surfaces as the first word of the message
/// (`"{handler_name} follower re-execute diverges from leader: ..."`)
/// so operational log scanning can identify which handler's
/// divergence fired without decoding the reply.
///
/// `reason` is a free-form description of the divergence (typically
/// `"hash mismatch (fresh={}, cached={})"` from
/// `verify_reply_hash_matches_cached`).  Appended after the
/// canonical prefix so the whole message carries both the handler
/// and the specific cause.
pub fn consensus_divergence_reply(handler_name: &str, reason: impl std::fmt::Display) -> Par {
    response::err(
        FSERR_CONSENSUS_DIVERGENCE,
        format!("{handler_name} follower re-execute diverges from leader: {reason}"),
    )
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    /// LOAD-BEARING wire-identity pin: `consensus_divergence_reply`
    /// produces a Par byte-identical to the pre-trait inline
    /// `response::err(FSERR_CONSENSUS_DIVERGENCE, format!(...))`
    /// pattern.
    ///
    /// # Consensus surface
    ///
    /// Under Wave 6, a verifying follower on
    /// `JournalPath::VerifyDivergence` sends this reply to the
    /// caller's ack channel.  A silent change in the reply bytes
    /// (different FSERR code, different message prefix, different
    /// positional layout) would make the follower's divergence
    /// reply disagree with the leader's agreement on what counts
    /// as a divergence — a consensus-observable regression that
    /// would split peering.
    ///
    /// # Operational surface
    ///
    /// A monitoring layer greps the "follower re-execute diverges
    /// from leader" substring to alert on divergence rates.  A
    /// drift in the message shape would silently break alerting.
    #[test]
    fn consensus_divergence_reply_matches_pre_trait_format() {
        for handler in &["fs_read", "fs_write", "fs_chmod", "fs_remove_dir"] {
            let reason = "hash mismatch (fresh=abc123, cached=def456)";
            let via_wrapper = consensus_divergence_reply(handler, reason);
            let via_pre_trait = response::err(
                FSERR_CONSENSUS_DIVERGENCE,
                format!("{handler} follower re-execute diverges from leader: {reason}"),
            );
            assert_eq!(
                via_wrapper.encode_to_vec(),
                via_pre_trait.encode_to_vec(),
                "wire-identity drift: consensus_divergence_reply must produce \
                 a Par byte-identical to the pre-trait pattern.  handler={handler}, \
                 reason={reason}",
            );
        }
    }

    /// Empty-reason acceptance pin — the helper's `impl Display`
    /// bound accepts a bare `&str`.  Pin that passing an empty
    /// reason still produces a well-formed reply (just the
    /// handler-name prefix without a trailing cause).  A regression
    /// that panicked on empty input would surface here.
    #[test]
    fn consensus_divergence_reply_accepts_empty_reason() {
        let reply = consensus_divergence_reply("fs_test", "");
        // Not vacuous — the Par is non-default (carries the
        // handler-name prefix) even with an empty reason.
        assert_ne!(reply.encode_to_vec(), Par::default().encode_to_vec());
    }

    /// `impl Display` bound accepts non-&str reasons — pin a
    /// `format_args!`-style call via a wrapper type.  Catches a
    /// regression that tightened the bound to `impl AsRef<str>`
    /// (would reject non-string Display impls like integers or
    /// custom error types that the yet-to-land verify.rs helper
    /// may return).
    #[test]
    fn consensus_divergence_reply_accepts_display_impl() {
        struct DisplayReason(u32);
        impl std::fmt::Display for DisplayReason {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "code={}", self.0)
            }
        }
        let via_display = consensus_divergence_reply("fs_test", DisplayReason(42));
        let via_str = consensus_divergence_reply("fs_test", "code=42");
        assert_eq!(
            via_display.encode_to_vec(),
            via_str.encode_to_vec(),
            "Display impl must produce the same reply as the equivalent &str",
        );
    }

    /// Handler-name surfaces in the reply message — a regression
    /// that dropped the prefix (e.g., refactored to a context-free
    /// message) would break operational alerting that correlates
    /// divergences to specific handlers.  Extract the message
    /// through the Par wire shape and verify the prefix survives.
    #[test]
    fn consensus_divergence_reply_includes_handler_name_in_message() {
        use crate::rust::interpreter::io::response::extract_err_code;

        // The reply is `[false, FSERR_CONSENSUS_DIVERGENCE, msg]` —
        // the extractor returns the code string, not the msg.  Pin
        // the code first (sanity check that we built the right
        // FSERR class).
        let reply = consensus_divergence_reply("fs_my_handler", "any-reason");
        let code = extract_err_code(&[reply.clone()]).expect("err code");
        assert_eq!(code, FSERR_CONSENSUS_DIVERGENCE.as_str());

        // Byte-level pin on the full reply shape — matches
        // `response::err(FSERR_CONSENSUS_DIVERGENCE, "fs_my_handler
        // follower re-execute diverges from leader: any-reason")`.
        let expected = response::err(
            FSERR_CONSENSUS_DIVERGENCE,
            "fs_my_handler follower re-execute diverges from leader: any-reason",
        );
        assert_eq!(reply.encode_to_vec(), expected.encode_to_vec());
    }

    /// Explicit substring pin for the operational grep surface.
    /// Byte-identity tests (above) catch any change to this
    /// substring transitively, but this test documents the surface
    /// directly: monitoring layers key on the exact phrase
    /// `"follower re-execute diverges from leader"` and a silent
    /// rewording would break alerting even if wire bytes shifted
    /// identically.  Encode the Par to protobuf and scan the raw
    /// bytes for the substring.
    #[test]
    fn consensus_divergence_reply_message_contains_operational_grep_substring() {
        const GREP_PHRASE: &str = "follower re-execute diverges from leader";
        let reply = consensus_divergence_reply("fs_test", "some-reason");
        let encoded = reply.encode_to_vec();
        // Scan the Par's encoded bytes for the ASCII substring.
        // The reply embeds the message as a protobuf-encoded
        // `GString`, which stores the raw UTF-8 bytes inline.
        let found = encoded
            .windows(GREP_PHRASE.len())
            .any(|w| w == GREP_PHRASE.as_bytes());
        assert!(
            found,
            "operational-grep regression: the \"{GREP_PHRASE}\" substring \
             is missing from the encoded reply.  A monitoring layer that \
             alerts on divergence rates by grepping this exact phrase \
             would silently stop firing.",
        );
    }
}
