// Handler family — groups fs_* handlers by effect shape.  Used by:
//
//   - The per-family count pin
//     `fs_handlers::fs_handlers_per_family_counts_match_pinned`
//     (slice 5.51) which locks the breakdown against the shape in
//     `EXPECTED_PER_FAMILY_HANDLER_COUNTS` so a copy-paste
//     miscategorization surfaces at test time.
//   - The per-family verifying count pin
//     `fs_handlers::fs_handlers_per_family_verifying_counts_match_pinned`
//     (slice 5.53) which locks the per-family VERIFYING split.
//   - The per-family handler file split under
//     `handlers/{mutation,observation,stream,lock,lifecycle}/*.rs`
//     — each handler lives under the subdirectory that names its
//     family, so the per-family count must match the per-directory
//     file count.
//
// Taxonomy is Wave 6 consensus-aware: the Mutation vs. Observation
// split determines WAL-journaling defaults; the Lifecycle family
// drives fd/handle-table operations that have replay side-effects.
// A regression that mis-labeled a handler would re-order journaling
// vs. side-effect timing on replay — consensus observable.

/// Groups fs_* handlers by effect shape on state and on the WAL.
///
///   - [`Mutation`](Self::Mutation) — writes state.  Includes
///     `fs_write`, `fs_write_at`, `fs_truncate`, `fs_chmod`,
///     `fs_chown`, `fs_remove_file`, `fs_rename`, `fs_copy_file`
///     (8 handlers at migration-complete, plus `fs_remove_dir`
///     trait-exempt for a one-off divergence-reply shape).
///     WAL-journaling by default; many (fs_chmod / fs_rename /
///     fs_copy_file / fs_truncate / fs_write*) verify replies
///     across leader / follower.
///
///   - [`Observation`](Self::Observation) — reads state without
///     mutating.  Includes `fs_read`, `fs_read_at`, `fs_stat`,
///     `fs_entries`, `fs_size`, `fs_seek`, `fs_exists`, `fs_flush`,
///     `fs_tell` (9 handlers).  Some (fs_stat / fs_entries /
///     fs_exists / fs_size) verify replies; fs_read / fs_read_at /
///     fs_seek are shape-observation-only and don't verify.
///
///   - [`Stream`](Self::Stream) — per-fd directory-entries
///     streaming primitives.  `fs_entries_stream_open`,
///     `fs_entries_stream_next`, `fs_entries_stream_close`
///     (3 handlers).  All three declare `const VERIFYING = false`
///     — streaming replies depend on per-call host-fd state (which
///     next-entry the kernel surfaces) so the leader/follower
///     reply hashes aren't by-construction equal.  Pinned by
///     `fs_handlers::EXPECTED_PER_FAMILY_VERIFYING_COUNTS[Stream]
///     = 0`.
///
///   - [`Lock`](Self::Lock) — byte-range and sequential lock
///     helpers.  `fs_lock_range`, `fs_lock_sequential`,
///     `fs_release_lock`, `fs_release_all_for_holder`
///     (4 handlers).  Non-verifying — the lock registry is
///     host-local.
///
///   - [`Lifecycle`](Self::Lifecycle) — file / cap creation +
///     retirement.  `fs_open`, `fs_close`, `fs_quarantine`
///     (3 handlers).  Non-verifying; `fs_open` installs a shadow
///     fd on replay via `on_replay_side_effect`.
///
/// Total at migration-complete: `8 + 9 + 3 + 4 + 3 = 27` migrated
/// handlers + 1 trait-exempt (`fs_remove_dir`) = 28 fs_* natives.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum HandlerFamily {
    Mutation,
    Observation,
    Stream,
    Lock,
    Lifecycle,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Variant ordering is a hidden surface — the per-family count
    /// pin (slice 5.51) iterates
    /// `EXPECTED_PER_FAMILY_HANDLER_COUNTS` which declares
    /// variants in this order.  Pin here that nothing has been
    /// inserted or re-ordered.
    #[test]
    fn variants_match_declaration_order() {
        assert_eq!(HandlerFamily::Mutation as u8, 0);
        assert_eq!(HandlerFamily::Observation as u8, 1);
        assert_eq!(HandlerFamily::Stream as u8, 2);
        assert_eq!(HandlerFamily::Lock as u8, 3);
        assert_eq!(HandlerFamily::Lifecycle as u8, 4);
    }

    /// Variant count pin — a sixth variant added without updating
    /// the per-family count pin (slice 5.51) would silently escape
    /// the breakdown.  Caught at test-time here first.
    #[test]
    fn exactly_five_variants() {
        // Exhaustive match proves variant count without runtime
        // introspection.
        let variants = [
            HandlerFamily::Mutation,
            HandlerFamily::Observation,
            HandlerFamily::Stream,
            HandlerFamily::Lock,
            HandlerFamily::Lifecycle,
        ];
        for v in variants {
            // Exhaustive pattern — a sixth variant would trip
            // the non-exhaustive-pattern compile error here.
            match v {
                HandlerFamily::Mutation
                | HandlerFamily::Observation
                | HandlerFamily::Stream
                | HandlerFamily::Lock
                | HandlerFamily::Lifecycle => {}
            }
        }
        assert_eq!(variants.len(), 5);
    }

    /// Equality + copy semantics — handlers store family in the
    /// `FsHandlerEntry.family: HandlerFamily` field; the per-family
    /// count pin (slice 5.51) compares by equality.  `Copy` keeps
    /// the comparison allocation-free.
    #[test]
    fn copy_and_eq_round_trip() {
        let m = HandlerFamily::Mutation;
        let m2 = m;
        assert_eq!(m, m2);
        assert_ne!(m, HandlerFamily::Observation);
    }
}
