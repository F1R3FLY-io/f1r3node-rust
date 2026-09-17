use std::collections::HashSet;

use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;

/// A slash system deploy whose containing chain was rejected by the merge.
/// The block creator uses this to re-issue the slash in the merge block
/// itself, ensuring the slash effect lands in the merged state regardless
/// of cost-optimal rejection of the source block's chain.
#[derive(Clone, Debug)]
pub struct RejectedSlash {
    pub invalid_block_hash: BlockHash,
    pub issuer_public_key: PublicKey,
    pub source_block_hash: BlockHash,
}

impl PartialEq for RejectedSlash {
    fn eq(&self, other: &Self) -> bool {
        self.invalid_block_hash == other.invalid_block_hash
            && self.issuer_public_key.bytes == other.issuer_public_key.bytes
    }
}

impl Eq for RejectedSlash {}

impl std::hash::Hash for RejectedSlash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.invalid_block_hash.hash(state);
        self.issuer_public_key.bytes.hash(state);
    }
}

/// Order a freshly-extracted `RejectedSlash` list for deterministic
/// downstream use. The extraction site builds the list by iterating a
/// `HashMap` keyed on the source block hash, so its order is
/// non-deterministic; the list is then cached in `MergedPreState` and
/// drives the order of the re-issued recovered slash deploys in the
/// proposed block body. Sorting by `(invalid_block_hash,
/// source_block_hash)` makes that order a function of the merge inputs
/// alone. The pair is unique per entry: the extractor emits at most one
/// slash per `invalid_block_hash` per source block.
pub fn sorted_for_body(mut slashes: Vec<RejectedSlash>) -> Vec<RejectedSlash> {
    slashes.sort_by(|a, b| {
        a.invalid_block_hash
            .cmp(&b.invalid_block_hash)
            .then_with(|| a.source_block_hash.cmp(&b.source_block_hash))
    });
    slashes
}

/// Filter rejected slashes for re-issuance by the merge proposer.
///
/// Two collapses happen here:
///
/// 1. **Drop already-covered equivocators.** If the proposer's own
///    slashing pass (`prepare_slashing_deploys`) already produces a
///    slash for the equivocator V, drop any merge-rejected slash for V.
///    The own-detected slash will land in the merge block — re-issuing
///    a redundant copy under the proposer's identity inflates body
///    size and wastes execution on a no-op slash (PoS slash is
///    keyed solely on `invalid_block_hash`, so a second slash for V
///    succeeds idempotently with no state change).
///
/// 2. **Dedup multiple rejected slashes for the same equivocator.**
///    When V1 and V2 both proposed slashes for V and both chains were
///    merge-rejected, the merge engine surfaces two `RejectedSlash`
///    entries — one per original issuer. The proposer is going to
///    re-sign both under their own identity, which would emit two
///    redundant SlashDeploys. Keep at most one survivor per
///    `invalid_block_hash`.
///
/// Output is sorted by `invalid_block_hash` so the surviving slash
/// for each equivocator is the same on every validator that runs the
/// same merge — required for body-hash determinism across replays.
pub fn filter_recoverable<I>(
    mut rejected: Vec<RejectedSlash>,
    own_invalid_block_hashes: I,
) -> Vec<RejectedSlash>
where
    I: IntoIterator<Item = BlockHash>,
{
    let covered: HashSet<BlockHash> = own_invalid_block_hashes.into_iter().collect();
    rejected.sort_by(|a, b| a.invalid_block_hash.cmp(&b.invalid_block_hash));
    let mut seen: HashSet<BlockHash> = HashSet::new();
    rejected
        .into_iter()
        .filter(|rs| !covered.contains(&rs.invalid_block_hash))
        .filter(|rs| seen.insert(rs.invalid_block_hash.clone()))
        .collect()
}

pub fn filter_recoverable_with_evidence<I, F, E>(
    rejected: Vec<RejectedSlash>,
    own_invalid_block_hashes: I,
    mut evidence_is_current: F,
) -> Result<Vec<RejectedSlash>, E>
where
    I: IntoIterator<Item = BlockHash>,
    F: FnMut(&BlockHash) -> Result<bool, E>,
{
    let mut out = Vec::new();
    for rs in filter_recoverable(rejected, own_invalid_block_hashes) {
        if evidence_is_current(&rs.invalid_block_hash)? {
            out.push(rs);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn pk(byte: u8) -> PublicKey { PublicKey::from_bytes(&[byte; 32]) }

    fn mk_slash(invalid_block_marker: u8, issuer_marker: u8) -> RejectedSlash {
        RejectedSlash {
            invalid_block_hash: Bytes::from(vec![invalid_block_marker; 32]),
            issuer_public_key: pk(issuer_marker),
            source_block_hash: Bytes::from(vec![0xFF; 32]),
        }
    }

    /// If the proposer's own slashing pass covers equivocator V, the
    /// merge-rejected slash for V is dropped — preventing two SlashDeploys
    /// for V (one own-detected, one re-issued) in the same block body.
    #[test]
    fn own_detected_slash_covers_merge_rejected_duplicate() {
        let rejected = vec![mk_slash(1, 2)];
        let own_invalid_block_hashes = std::iter::once(Bytes::from(vec![1u8; 32]));
        let out = filter_recoverable(rejected, own_invalid_block_hashes);
        assert!(
            out.is_empty(),
            "merge-rejected slash duplicating own slash must be dropped"
        );
    }

    /// A merge-rejected slash for an equivocator that the proposer's own
    /// `invalid_latest_messages` view does NOT cover must survive dedup
    /// and be re-issued in the merge block. Without this, an attacker who
    /// sustains cheap conflicts could starve slashing indefinitely.
    #[test]
    fn merge_rejected_slash_survives_when_not_covered_by_own() {
        let rejected = vec![mk_slash(1, 2)];
        let own_invalid_block_hashes: Vec<BlockHash> = vec![];
        let out = filter_recoverable(rejected, own_invalid_block_hashes);
        assert_eq!(out.len(), 1, "merge-rejected slash must survive dedup");
        assert_eq!(out[0].invalid_block_hash, Bytes::from(vec![1u8; 32]));
    }

    /// When multiple merge-rejected slashes refer to distinct equivocators,
    /// all uncovered ones must survive dedup, in deterministic order.
    /// Covered equivocators are dropped.
    #[test]
    fn mixed_coverage_keeps_uncovered_slashes() {
        let rejected = vec![
            mk_slash(1, 2), // covered by own
            mk_slash(3, 4), // not covered
            mk_slash(5, 6), // not covered
            mk_slash(7, 8), // covered by own
        ];
        let own_invalid_block_hashes = vec![Bytes::from(vec![1u8; 32]), Bytes::from(vec![7u8; 32])];
        let out = filter_recoverable(rejected, own_invalid_block_hashes);
        assert_eq!(out.len(), 2, "exactly the uncovered slashes must survive");
        // Sorted by invalid_block_hash for deterministic body composition.
        assert_eq!(out[0].invalid_block_hash, Bytes::from(vec![3u8; 32]));
        assert_eq!(out[1].invalid_block_hash, Bytes::from(vec![5u8; 32]));
    }

    /// Two merge-rejected slashes for the SAME equivocator from DIFFERENT
    /// original issuers (e.g., V1 and V2 both proposed slash chains for
    /// equivocator V and both got merge-rejected) must collapse to a
    /// single survivor. The proposer is going to re-sign whichever
    /// survives under their own identity, so emitting two SlashDeploys
    /// for V is wasted work — the second execution is a no-op against
    /// already-slashed PoS state.
    #[test]
    fn same_equivocator_across_issuers_dedups_to_one() {
        let rejected = vec![mk_slash(1, 2), mk_slash(1, 3)];
        let own_invalid_block_hashes: Vec<BlockHash> = vec![];
        let out = filter_recoverable(rejected, own_invalid_block_hashes);
        assert_eq!(
            out.len(),
            1,
            "two rejected slashes for the same equivocator must collapse to one"
        );
        assert_eq!(out[0].invalid_block_hash, Bytes::from(vec![1u8; 32]));
    }

    /// The proposer's own slash for E must drop a merge-rejected slash
    /// for E even when the rejected slash's original issuer is a
    /// different validator. The proposer re-signs every E1c slash under
    /// their own pk anyway, so the original issuer is provenance, not
    /// dedup-key material.
    #[test]
    fn own_detection_drops_rejected_from_other_issuer() {
        let rejected = vec![mk_slash(1, 99)]; // issuer = other validator
        let own_invalid_block_hashes = std::iter::once(Bytes::from(vec![1u8; 32])); // own slashes E
        let out = filter_recoverable(rejected, own_invalid_block_hashes);
        assert!(
            out.is_empty(),
            "own-detected slash for E must drop ANY merge-rejected slash for E, \
             regardless of original issuer. If this fails, dedup is keying on \
             issuer pk and will produce redundant SlashDeploys in the merge block body."
        );
    }

    /// Empty inputs produce empty output — the common non-slash merge path.
    /// Regression guard: block creators with no rejected slashes must
    /// return an empty list rather than panic or allocate spuriously.
    #[test]
    fn empty_inputs_produce_empty_output() {
        let out = filter_recoverable(Vec::<RejectedSlash>::new(), Vec::<BlockHash>::new());
        assert!(out.is_empty());
    }

    /// `sorted_for_body` must impose a total order that depends only on
    /// the merge inputs, not on the `HashMap` iteration order the
    /// extractor happens to produce. Same set in, same order out, with
    /// `source_block_hash` breaking ties on `invalid_block_hash`.
    #[test]
    fn sorted_for_body_orders_by_invalid_then_source_block() {
        let mk = |invalid: u8, source: u8| RejectedSlash {
            invalid_block_hash: Bytes::from(vec![invalid; 32]),
            issuer_public_key: pk(0),
            source_block_hash: Bytes::from(vec![source; 32]),
        };
        let a = mk(1, 9);
        let b = mk(1, 3);
        let c = mk(5, 1);

        let one = sorted_for_body(vec![c.clone(), a.clone(), b.clone()]);
        let two = sorted_for_body(vec![b.clone(), c.clone(), a.clone()]);

        let key = |s: &RejectedSlash| (s.invalid_block_hash.to_vec(), s.source_block_hash.to_vec());
        assert_eq!(
            one.iter().map(key).collect::<Vec<_>>(),
            two.iter().map(key).collect::<Vec<_>>(),
            "sorted_for_body output must not depend on input order"
        );
        assert_eq!(key(&one[0]), key(&b));
        assert_eq!(key(&one[1]), key(&a));
        assert_eq!(key(&one[2]), key(&c));
    }

    #[test]
    fn non_current_rejected_slash_is_not_recovered() {
        let rejected = vec![mk_slash(1, 2), mk_slash(2, 3)];
        let out = filter_recoverable_with_evidence(rejected, Vec::<BlockHash>::new(), |h| {
            Ok::<bool, ()>(h.as_ref()[0] == 1)
        })
        .expect("evidence filter should not fail");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].invalid_block_hash, Bytes::from(vec![1u8; 32]));
    }

    #[test]
    fn rejected_slash_evidence_filter_errors_propagate() {
        let rejected = vec![mk_slash(1, 2)];
        let out = filter_recoverable_with_evidence(rejected, Vec::<BlockHash>::new(), |_h| {
            Err::<bool, &str>("lookup failed")
        });
        assert_eq!(out, Err("lookup failed"));
    }
}
