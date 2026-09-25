pub mod errors;
pub mod mode;
pub mod nss;
pub mod response;

/// Consensus vs. oracular execution mode.
///
/// Threaded from `ProcessContext` into every path-taking handler.  Under
/// `Consensus`, host-transient fields (`mtime`, `ctime`, `atime`, `owner`,
/// `group`) are omitted from `stat` / `entries` records, and `chown`
/// returns `FSERR_UNSUPPORTED`.
///
/// `Default` returns `Consensus` — the more restrictive mode — so any
/// construction site that omits the mode fails closed rather than
/// silently allowing chown and leaking host metadata.  All shipping
/// call sites should be explicit; this default only matters for future
/// refactors / test scaffolds that use `..Default::default()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConsensusMode {
    Oracular,
    #[default]
    Consensus,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fail-closed default pin: any refactor that swaps the
    /// `#[default]` to `Oracular` is a security regression — a
    /// construction site that omits the mode would silently allow
    /// `chown` and leak host metadata.
    #[test]
    fn consensus_mode_default_is_consensus() {
        assert_eq!(ConsensusMode::default(), ConsensusMode::Consensus);
    }
}
