// Discriminates the four framework paths at which a verifying
// handler's `journal` method will fire (the dispatcher's framework
// loop is yet to land, slice 4.6).  Handlers pattern-match on the
// path to select:
//
//   - The correct WAL-entry shape — fs_read's success-vs-divergence
//     entries differ in `payload_ref`.
//   - The correct state-advance source — fs_read advances its
//     shadow position by the ACTUALLY-read bytes even on
//     verify-divergence, when the produced reply is the divergence
//     error.
//
// The reply-source distinction (`produce_reply` vs.
// `state_source_reply`) is the subtle invariant: on
// `VerifyDivergence` the two diverge — the handler produces the
// divergence reply to the caller but journals / advances state
// from the fresh syscall reply.  A regression that collapsed the
// two methods would under-count state advance on divergence paths
// (fs_read shadow position drift, consensus-observable under Wave 6).

use models::rhoapi::Par;

/// Discriminates the four framework paths at which
/// [`crate::rust::interpreter::io::handler_trait::reply::HandlerReply`]-producing
/// handlers' `journal` methods fire.  Carried by the yet-to-land
/// dispatcher framework (slice 4.6) into every journaling handler's
/// `journal` call.
pub enum JournalPath<'a> {
    /// `is_replay = false` — leader path after successful dispatch.
    /// `fresh_reply` is what will be produced to ack.
    Leader { fresh_reply: &'a Par },

    /// `is_replay = true` + verifying handler + Consensus cmode +
    /// `verify_reply_hash_matches_cached` succeeded.  `fresh_reply`
    /// is what will be produced.
    VerifySuccess { fresh_reply: &'a Par },

    /// `is_replay = true` + verifying handler + Consensus cmode +
    /// `verify_reply_hash_matches_cached` failed.
    /// `divergence_reply` is what will be produced; `fresh_reply`
    /// is the actual syscall outcome retained for state-advance
    /// purposes (fs_read shadow position etc.).
    VerifyDivergence {
        fresh_reply: &'a Par,
        divergence_reply: &'a Par,
    },

    /// Oracular tautological echo — either a non-verifying handler
    /// on `is_replay = true`, or a verifying handler whose cmode
    /// resolves to Oracular / unresolved.  `previous_reply` is
    /// `previous.first()` (the leader's cached play-time reply).
    OracularEcho { previous_reply: &'a Par },
}

impl<'a> JournalPath<'a> {
    /// The reply Par that will be produced to ack.  Handlers that
    /// journal the produced reply (fs_stat, fs_size, fs_exists)
    /// use this.
    ///
    /// On `VerifyDivergence` this returns `divergence_reply` — the
    /// framework-built consensus-divergence reply — not the fresh
    /// syscall reply.  Use [`state_source_reply`](Self::state_source_reply)
    /// when the fresh outcome is the handler's state-advance source.
    #[must_use]
    pub fn produce_reply(&self) -> &'a Par {
        match *self {
            JournalPath::Leader { fresh_reply } => fresh_reply,
            JournalPath::VerifySuccess { fresh_reply } => fresh_reply,
            JournalPath::VerifyDivergence {
                divergence_reply, ..
            } => divergence_reply,
            JournalPath::OracularEcho { previous_reply } => previous_reply,
        }
    }

    /// The reply Par whose bytes / count drive shadow-state
    /// advance.  Same as [`produce_reply`](Self::produce_reply)
    /// EXCEPT on `VerifyDivergence`, where this returns the FRESH
    /// syscall reply (actual bytes read / written) rather than the
    /// divergence-err reply.
    ///
    /// Under Wave 6, a regression that collapsed these two methods
    /// would make fs_read on verify-divergence advance its shadow
    /// position by zero (list length of the err reply) rather than
    /// by the actually-read bytes — a leader / follower drift that
    /// would snowball across subsequent reads.
    #[must_use]
    pub fn state_source_reply(&self) -> &'a Par {
        match *self {
            JournalPath::Leader { fresh_reply } => fresh_reply,
            JournalPath::VerifySuccess { fresh_reply } => fresh_reply,
            JournalPath::VerifyDivergence { fresh_reply, .. } => fresh_reply,
            JournalPath::OracularEcho { previous_reply } => previous_reply,
        }
    }

    /// True iff this path is a verify-failure.  Handlers whose
    /// divergence-journal shape differs from their success-journal
    /// shape (fs_read: `journal_read_divergence` vs. `journal_read`)
    /// branch on this.
    pub fn is_divergence(&self) -> bool { matches!(self, JournalPath::VerifyDivergence { .. }) }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
    use crate::rust::interpreter::io::response;

    fn par_fresh() -> Par { response::ok_u64(42) }
    fn par_div() -> Par { response::err(FSERR_BAD_ARG, "divergence") }
    fn par_prev() -> Par { response::ok_u64(99) }

    /// `Leader`: `produce_reply` and `state_source_reply` both
    /// return `fresh_reply`.
    #[test]
    fn leader_both_methods_return_fresh_reply() {
        let fresh = par_fresh();
        let path = JournalPath::Leader {
            fresh_reply: &fresh,
        };
        assert_eq!(path.produce_reply().encode_to_vec(), fresh.encode_to_vec());
        assert_eq!(
            path.state_source_reply().encode_to_vec(),
            fresh.encode_to_vec()
        );
        assert!(!path.is_divergence());
    }

    /// `VerifySuccess`: same as `Leader` — both methods return
    /// `fresh_reply`.
    #[test]
    fn verify_success_both_methods_return_fresh_reply() {
        let fresh = par_fresh();
        let path = JournalPath::VerifySuccess {
            fresh_reply: &fresh,
        };
        assert_eq!(path.produce_reply().encode_to_vec(), fresh.encode_to_vec());
        assert_eq!(
            path.state_source_reply().encode_to_vec(),
            fresh.encode_to_vec()
        );
        assert!(!path.is_divergence());
    }

    /// LOAD-BEARING: on `VerifyDivergence`, `produce_reply` returns
    /// the divergence reply while `state_source_reply` returns the
    /// fresh syscall reply.  A regression that collapsed these two
    /// (e.g., by making `state_source_reply` delegate to
    /// `produce_reply`) would make verifying handlers on the
    /// divergence path under-advance shadow state — a Wave 6
    /// consensus bug.
    #[test]
    fn verify_divergence_distinguishes_produce_and_state_source() {
        let fresh = par_fresh();
        let div = par_div();
        let path = JournalPath::VerifyDivergence {
            fresh_reply: &fresh,
            divergence_reply: &div,
        };

        assert_eq!(path.produce_reply().encode_to_vec(), div.encode_to_vec());
        assert_eq!(
            path.state_source_reply().encode_to_vec(),
            fresh.encode_to_vec()
        );
        assert!(path.is_divergence());

        // Not vacuous — the two Pars differ.
        assert_ne!(fresh.encode_to_vec(), div.encode_to_vec());
    }

    /// `OracularEcho`: both methods return `previous_reply`.
    #[test]
    fn oracular_echo_both_methods_return_previous_reply() {
        let prev = par_prev();
        let path = JournalPath::OracularEcho {
            previous_reply: &prev,
        };
        assert_eq!(path.produce_reply().encode_to_vec(), prev.encode_to_vec());
        assert_eq!(
            path.state_source_reply().encode_to_vec(),
            prev.encode_to_vec()
        );
        assert!(!path.is_divergence());
    }

    /// `is_divergence` matches EXACTLY one variant.  A refactor
    /// that broadened it (e.g., to include `VerifySuccess`) would
    /// make handlers' divergence-shape branches run on success
    /// paths — a consensus-observable journal-shape regression.
    #[test]
    fn is_divergence_matches_exactly_verify_divergence() {
        let fresh = par_fresh();
        let div = par_div();
        let prev = par_prev();

        assert!(!JournalPath::Leader {
            fresh_reply: &fresh
        }
        .is_divergence());
        assert!(!JournalPath::VerifySuccess {
            fresh_reply: &fresh
        }
        .is_divergence());
        assert!(JournalPath::VerifyDivergence {
            fresh_reply: &fresh,
            divergence_reply: &div,
        }
        .is_divergence());
        assert!(!JournalPath::OracularEcho {
            previous_reply: &prev
        }
        .is_divergence());
    }
}
