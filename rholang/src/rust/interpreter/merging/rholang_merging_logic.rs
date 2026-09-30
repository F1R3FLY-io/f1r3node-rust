// See rholang/src/main/scala/coop/rchain/rholang/interpreter/merging/RholangMergingLogic.scala

use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use indexmap::IndexSet;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieInsertAction, TrieInsertBinaryProduce,
};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
use rspace_plus_plus::rspace::merger::merging_logic::{
    apply_mergeable_value, mergeable_value_diff, MergeType,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::serializers::serializers;
use rspace_plus_plus::rspace::trace::event::Produce;

use crate::rust::interpreter::rho_type::{RhoBigInt, RhoNumber};

pub struct RholangMergingLogic;

impl RholangMergingLogic {
    /**
     * Transforms absolute values with the difference from initial values.
     *
     * Example for 3 state changes (A, B, C are channels, PSH is initial value/pre-state hash):
     *
     * Initial state (PSH):
     *   A = 10, B = 2, C = 20
     *
     * Final values:      Calculated diffs:
     * Change 0: A = 20   A = +10
     * Change 1: B = 5    B = +3
     * Change 2: A = 15   A = -5
     *           C = 10   C = -10
     *
     * @param channelValues Final values
     * @param getInitialValue Accessor to initial value
     */
    pub fn calculate_num_channel_diff<Key: Clone + Eq + Hash + Ord>(
        channel_values: Vec<BTreeMap<Key, (BigInt, MergeType)>>,
        get_initial_value: impl Fn(&Key) -> Option<BigInt> + Send + Sync,
    ) -> Vec<BTreeMap<Key, (BigInt, MergeType)>> {
        // First collect unique keys while preserving order
        let unique_keys: Vec<_> = channel_values
            .iter()
            .flat_map(|channel| channel.keys().cloned())
            .collect::<IndexSet<_>>()
            .into_iter()
            .collect();

        let mut state = unique_keys
            .iter()
            .map(|key| (key.clone(), get_initial_value(key).unwrap_or_default()))
            .collect::<BTreeMap<_, _>>();

        // Process each channel value map
        channel_values
            .into_iter()
            .map(|end_val_map| {
                let mut diffs = BTreeMap::new();

                for (ch, (end_val, merge_type)) in end_val_map {
                    if let Some(prev_val) = state.get(&ch) {
                        // IntegerAdd uses i64 wrapping_sub DELIBERATELY: it is the exact
                        // group inverse of the wrapping add that language-level execution
                        // (reduce.rs GInt `+`, intended 64-bit semantics) used to produce
                        // `end_val`. So the diff recovers the deploy's TRUE intended delta
                        // even when execution overflowed and stored a wrapped `end_val`.
                        // An over-large delta is rejected DOWNSTREAM — at combine
                        // (rspace++ combine_mergeable_value) and at the terminal apply
                        // (calculate_number_channel_merge) — never here; erroring here
                        // would crash block processing on a deploy that must instead be
                        // gracefully rejected at merge time. BigIntAdd is exact.
                        let diff = mergeable_value_diff(prev_val, &end_val, merge_type);
                        diffs.insert(ch.clone(), (diff, merge_type));
                        state.insert(ch, end_val);
                    }
                }
                diffs
            })
            .collect()
    }

    /**
     * Merge number channel value from multiple changes and base state.
     *
     * @param channelHash Channel hash
     * @param diff Difference from base state
     * @param changes Channel changes to calculate new random generator
     * @param getBaseData Base state value reader
     */
    pub fn calculate_number_channel_merge(
        channel_hash: &Blake2b256Hash,
        diff: &BigInt,
        merge_type: MergeType,
        changes: &ChannelChange<Vec<u8>>,
        get_base_data: impl Fn(&Blake2b256Hash) -> Result<Vec<Datum<ListParWithRandom>>, HistoryError>,
    ) -> Result<
        HotStoreTrieAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        HistoryError,
    > {
        // Read initial value of number channel from base state.
        // None = channel doesn't exist yet (treat as 0); Err = invariant
        // violation (non-numeric or multi-value pre-state) — propagate so the
        // merge is rejected rather than silently substituting 0.
        let init_num =
            Self::convert_to_read_number(get_base_data)(channel_hash)?.unwrap_or_default();
        // Terminal apply that WRITES the merged number-channel value. Mirror the
        // vault rule in conflict_set_merger::cal_merged_result (range + `>= 0`):
        // a wrapping_add here could silently commit an overflowed/negative balance
        // to consensus state. Reject loudly instead (defense-in-depth backstop —
        // the accepted branch set already passed the same gate at the combine step).
        let new_val = apply_mergeable_value(&init_num, diff, merge_type).ok_or_else(|| {
            HistoryError::MergeError(format!(
                "Number channel {:?} merge rejected: base {} + diff {} ({:?}) \
                 overflows the channel range or yields a negative balance",
                channel_hash, init_num, diff, merge_type,
            ))
        })?;

        // Calculate merged random generator (use only unique changes as input)
        let new_rnd = if changes.added.iter().collect::<HashSet<_>>().len() == 1 {
            // Single branch, just use available random generator
            Self::decode_rnd(changes.added.first().unwrap().to_vec())
        } else {
            // Multiple branches, merge random generators
            let rnd_added_sorted = changes
                .added
                .iter()
                .map(|bytes| Self::decode_rnd(bytes.to_vec()))
                .collect::<HashSet<_>>()
                .into_iter()
                .map(|rnd| (rnd.clone(), rnd.to_bytes()))
                .collect::<Vec<_>>();

            // Sort by bytes
            let mut sorted = rnd_added_sorted;
            sorted.sort_by(|a, b| a.1.cmp(&b.1));

            // Extract sorted random generators
            let sorted_rnds = sorted.into_iter().map(|(rnd, _)| rnd).collect::<Vec<_>>();

            // Merge the random generators
            Blake2b512Random::merge(sorted_rnds)
        };

        // Create final merged value
        let datum_encoded =
            Self::create_datum_encoded(channel_hash, &new_val, merge_type, new_rnd)?;

        // Create update store action
        Ok(HotStoreTrieAction::TrieInsertAction(
            TrieInsertAction::TrieInsertBinaryProduce(TrieInsertBinaryProduce {
                hash: channel_hash.clone(),
                data: vec![datum_encoded],
            }),
        ))
    }

    fn decode_rnd(par_with_rnd_encoded: Vec<u8>) -> Blake2b512Random {
        let datum: Datum<ListParWithRandom> = serializers::decode_datum(&par_with_rnd_encoded);

        Blake2b512Random::from_bytes(&datum.a.random_state)
    }

    /// §3c single-value-cell discriminator (RCA-asi-devnet-finality-halt).
    ///
    /// A channel whose base state is a single NUMERIC datum is a single-value
    /// (number) cell — even for a write this merge did not tag mergeable.
    /// Registry / TreeHashMap nodes hold structured (Map/tuple) data, which is
    /// non-numeric, so `try_get_number_with_rnd` returns None and they are
    /// exempt (multi-key structures merge freely). This is the discriminator
    /// that separates a purse/cell (conflict) from a registry (merge) among
    /// disjoint-consumed / produce-only writes, which the consume-then-produce
    /// conflict check cannot distinguish.
    ///
    /// Returns `Err` when applying `changes` to such a cell would leave it
    /// holding more than one value — a genuine single-value conflict the merge
    /// must reject, rather than persist a state the RhoVM rejects at read time
    /// (the IntegerAdd single-value invariant), which halts block production.
    pub fn check_single_value_cell_not_overfilled(
        channel_hash: &Blake2b256Hash,
        base_data: &[Datum<ListParWithRandom>],
        base_binary: &[Vec<u8>],
        changes: &ChannelChange<Vec<u8>>,
    ) -> Result<(), HistoryError> {
        // Only a produce (`added`) can grow the cell beyond its base cardinality.
        if changes.added.is_empty() {
            return Ok(());
        }
        let base_is_single_number =
            base_data.len() == 1 && Self::try_get_number_with_rnd(&base_data[0].a).is_some();
        let added_are_numbers = changes
            .added
            .iter()
            .all(|bytes| Self::encoded_datum_is_number(bytes));
        if !base_is_single_number && !(base_data.is_empty() && added_are_numbers) {
            return Ok(());
        }
        let kept = StateChange::multiset_diff(base_binary, &changes.removed);
        let result_len = kept.len() + changes.added.len();
        if result_len > 1 {
            return Err(HistoryError::MergeError(format!(
                "single-value cell {} would hold {} values after merge; concurrent \
                 writes to a single-value (number) channel conflict",
                hex::encode(channel_hash.clone().bytes()),
                result_len,
            )));
        }
        Ok(())
    }

    /// Returns the number + RNG pair for a single-Par `Int` or `BigInt`
    /// channel value, or None when the value isn't a single-Par number (e.g.,
    /// a Rholang Map on a registry leaf node tagged with the bitmask tag).
    /// Non-numeric values fall through to the existing conflict-rejection path
    /// rather than wedging the merger.
    pub fn try_get_number_with_rnd(
        par_with_rnd: &ListParWithRandom,
    ) -> Option<(BigInt, Blake2b512Random)> {
        if par_with_rnd.pars.len() != 1 {
            return None;
        }
        let par = &par_with_rnd.pars[0];
        RhoNumber::unapply(par)
            .map(BigInt::from)
            .or_else(|| RhoBigInt::unapply(par))
            .map(|num| {
                (
                    num,
                    Blake2b512Random::from_bytes(&par_with_rnd.random_state),
                )
            })
    }

    /// Merge strategy for a channel value under the strategy of its mergeable
    /// tag. Additive tags follow the value kind: `Int` merges as `IntegerAdd`
    /// (i64 range) and `BigInt` as `BigIntAdd`. `BitmaskOr` accepts only `Int`.
    /// Returns None when the value is not a number valid for the tag.
    pub fn number_merge_type(
        par_with_rnd: &ListParWithRandom,
        tag: MergeType,
    ) -> Option<MergeType> {
        if par_with_rnd.pars.len() != 1 {
            return None;
        }
        let par = &par_with_rnd.pars[0];
        let is_int = RhoNumber::unapply(par).is_some();
        let is_bigint = !is_int && RhoBigInt::unapply(par).is_some();
        match tag {
            MergeType::IntegerAdd | MergeType::BigIntAdd if is_int => Some(MergeType::IntegerAdd),
            MergeType::IntegerAdd | MergeType::BigIntAdd if is_bigint => Some(MergeType::BigIntAdd),
            MergeType::BitmaskOr if is_int => Some(MergeType::BitmaskOr),
            _ => None,
        }
    }

    pub fn encoded_datum_is_number(bytes: &[u8]) -> bool {
        bincode::deserialize::<Datum<ListParWithRandom>>(bytes)
            .ok()
            .and_then(|datum| Self::try_get_number_with_rnd(&datum.a))
            .is_some()
    }

    fn create_datum_encoded(
        channel_hash: &Blake2b256Hash,
        num: &BigInt,
        merge_type: MergeType,
        rnd: Blake2b512Random,
    ) -> Result<Vec<u8>, HistoryError> {
        // Create value with random generator
        let num_par = match merge_type {
            MergeType::BigIntAdd => RhoBigInt::create_par(num),
            MergeType::IntegerAdd | MergeType::BitmaskOr => {
                RhoNumber::create_par(num.to_i64().ok_or_else(|| {
                    HistoryError::MergeError(format!(
                        "Number channel {:?} merged value {} does not fit an Int ({:?})",
                        channel_hash, num, merge_type,
                    ))
                })?)
            }
        };
        let par_with_rnd = ListParWithRandom {
            pars: vec![num_par],
            random_state: rnd.to_bytes(),
        };

        // Create hash of the data
        let data_hash =
            stable_hash_provider::hash_produce(channel_hash.bytes(), &par_with_rnd, false);

        // Create produce
        let produce = Produce {
            channel_hash: channel_hash.clone(),
            hash: data_hash,
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        };

        // Create datum
        let datum = Datum {
            a: par_with_rnd,
            persist: false,
            source: produce,
        };

        // Encode datum
        Ok(serializers::encode_datum(&datum))
    }

    /// Adapter from a fallible channel-data reader to a fallible single-number
    /// reader. Three result cases:
    /// - `Ok(None)` — channel has no data (legitimate; treat downstream as 0).
    /// - `Ok(Some(n))` — channel holds a single numeric value.
    /// - `Err(_)` — invariant violation (multi-value pre-state, non-numeric
    ///   value where numeric expected) or upstream I/O error. Caller must
    ///   propagate to reject the merge rather than silently substitute 0.
    pub fn convert_to_read_number<F>(
        get_data_func: F,
    ) -> impl Fn(&Blake2b256Hash) -> Result<Option<BigInt>, HistoryError>
    where F: Fn(&Blake2b256Hash) -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> {
        move |hash: &Blake2b256Hash| {
            let data = get_data_func(hash)?;
            if data.len() > 1 {
                return Err(HistoryError::MergeError(format!(
                    "Number channel {:?} has {} pre-state values; single-value invariant violated",
                    hash,
                    data.len(),
                )));
            }
            match data.first() {
                None => Ok(None),
                Some(datum) => match Self::try_get_number_with_rnd(&datum.a) {
                    Some((n, _)) => Ok(Some(n)),
                    None => Err(HistoryError::MergeError(format!(
                        "Number channel {:?} pre-state value is non-numeric; \
                         channel-type invariant violated",
                        hash,
                    ))),
                },
            }
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct DeployMergeableData {
    pub channels: Vec<NumberChannel>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct NumberChannel {
    pub hash: Blake2b256Hash,
    pub diff: BigInt,
    pub merge_type: MergeType,
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/merging/RholangMergingLogicSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn num_datum(n: i64) -> Datum<ListParWithRandom> {
        let lpwr = ListParWithRandom {
            pars: vec![RhoNumber::create_par(n)],
            random_state: vec![0u8; 32],
        };
        Datum::create(&"chan".to_string(), lpwr, false)
    }

    fn non_numeric_datum() -> Datum<ListParWithRandom> {
        // Two Pars -> not a single-Par integer -> models a registry/Map leaf.
        let lpwr = ListParWithRandom {
            pars: vec![RhoNumber::create_par(1), RhoNumber::create_par(2)],
            random_state: vec![0u8; 32],
        };
        Datum::create(&"chan".to_string(), lpwr, false)
    }

    fn change(added: Vec<Vec<u8>>, removed: Vec<Vec<u8>>) -> ChannelChange<Vec<u8>> {
        ChannelChange { added, removed }
    }

    // The halt scenario: a single-value number cell (base [0]) receives a
    // produce that does NOT consume the base -> would hold [0, 5e9] -> reject.
    #[test]
    fn single_value_cell_produce_only_is_rejected() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let base = vec![num_datum(0)];
        let base_bin = vec![vec![0x00u8]];
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &base,
            &base_bin,
            &change(vec![vec![0x5eu8]], vec![]),
        );
        assert!(
            res.is_err(),
            "produce-only onto single-value number cell must be rejected"
        );
    }

    // A proper read-modify-write consumes the base and produces one replacement
    // -> stays single-valued -> allowed.
    #[test]
    fn single_value_cell_read_modify_write_is_allowed() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let base = vec![num_datum(0)];
        let base_bin = vec![vec![0x00u8]];
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &base,
            &base_bin,
            &change(vec![vec![0x5eu8]], vec![vec![0x00u8]]),
        );
        assert!(res.is_ok(), "RMW that consumes the base must be allowed");
    }

    #[test]
    fn empty_numeric_cell_multiple_produces_are_rejected() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let added_a = serializers::encode_datum(&num_datum(0));
        let added_b = serializers::encode_datum(&num_datum(0));
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &[],
            &[],
            &change(vec![added_a, added_b], vec![]),
        );
        assert!(res.is_err());
    }

    #[test]
    fn empty_numeric_cell_single_produce_is_allowed() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let added = serializers::encode_datum(&num_datum(0));
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &[],
            &[],
            &change(vec![added], vec![]),
        );
        assert!(res.is_ok());
    }

    // A non-numeric base (registry / TreeHashMap leaf) is a multi-key structure,
    // not a single-value cell -> concurrent produces merge freely.
    #[test]
    fn non_numeric_base_registry_merges_freely() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let base = vec![non_numeric_datum()];
        let base_bin = vec![vec![0x00u8]];
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &base,
            &base_bin,
            &change(vec![vec![0xaau8]], vec![]),
        );
        assert!(
            res.is_ok(),
            "registry/non-numeric channel must merge, not conflict"
        );
    }

    #[test]
    fn no_produce_is_allowed() {
        let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
        let base = vec![num_datum(0)];
        let base_bin = vec![vec![0x00u8]];
        let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
            &ch,
            &base,
            &base_bin,
            &change(vec![], vec![]),
        );
        assert!(res.is_ok());
    }

    // Property companion to the §3c unit tests above and to ConflictSoundness.v
    // Section Overfill: `check_single_value_cell_not_overfilled` rejects a merge
    // IFF the post-merge single-value cell would hold > 1 value. The guard's
    // `result_len = multiset_diff(base_binary, removed).len() + added.len()` is
    // the Rocq model's `cell_after = kept + added`, and the rejection predicate
    // `result_len > 1` is exactly `svc_guard_active`'s `1 <? cell_after`.
    mod svc_guard_property {
        use proptest::prelude::*;
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
        use rspace_plus_plus::rspace::merger::state_change::StateChange;

        use super::{change, num_datum, RholangMergingLogic};

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(400))]

            #[test]
            fn svc_guard_rejects_iff_result_len_gt_one(
                base_num in any::<i64>(),
                base_byte in any::<u8>(),
                removes_base in any::<bool>(),
                added_len in 0usize..4,
            ) {
                // A single-value NUMBER cell: exactly one numeric base datum with a
                // 1-element binary projection. `removes_base` selects a proper
                // read-modify-write (consumes the base) vs a produce-only write
                // (leaves it); `added_len` produces 0..3 new data.
                let ch = Blake2b256Hash::from_bytes(vec![0x0d; 32]);
                let base = vec![num_datum(base_num)];
                let base_bin = vec![vec![base_byte]];
                let removed: Vec<Vec<u8>> =
                    if removes_base { vec![vec![base_byte]] } else { vec![] };
                let added: Vec<Vec<u8>> =
                    (0..added_len).map(|i| vec![0xA0u8.wrapping_add(i as u8)]).collect();
                let changes = change(added.clone(), removed.clone());

                // cell_after computed EXACTLY as the guard does (same multiset_diff).
                let kept = StateChange::multiset_diff(&base_bin, &removed);
                let cell_after = kept.len() + added.len();

                let res = RholangMergingLogic::check_single_value_cell_not_overfilled(
                    &ch, &base, &base_bin, &changes,
                );
                // Reject IFF the merged cell would hold more than one value.
                // (added-empty short-circuits to Ok, and with a 1-element base_bin
                // that means cell_after <= 1, so the iff still holds there too.)
                prop_assert_eq!(
                    res.is_err(), cell_after > 1,
                    "guard must reject IFF the single-value cell would hold >1 value \
                     (cell_after={}, added={:?}, removed={:?})",
                    cell_after, added, removed
                );
            }
        }
    }

    #[test]
    fn test_calculate_num_channel_diff() {
        /*
         *        A   B   C        A   B   C
         *  ---------------       ----------
         *  PSH  10      20
         *
         *   0.  20               10
         *   1.       3      ==>       3
         *   2.  15      10       -5     -10
         */

        // Create string hashes for readability
        let ch_a = "A".to_string();
        let ch_b = "B".to_string();
        let ch_c = "C".to_string();

        // Define initial values
        let mut init_values = HashMap::new();
        init_values.insert(ch_a.clone(), 10i64);
        init_values.insert(ch_c.clone(), 20i64);

        // Define the accessor function to get initial values
        let get_data_on_hash =
            |hash: String| -> Option<BigInt> { init_values.get(&hash).copied().map(BigInt::from) };

        // Define input channel values (Vec of Maps); all entries use IntegerAdd
        // semantics for the existing vault path.
        let mt = MergeType::IntegerAdd;
        let mut input = Vec::new();

        // Map 0: {A -> 20}
        let mut map0 = BTreeMap::new();
        map0.insert(ch_a.clone(), (BigInt::from(20i64), mt));
        input.push(map0);

        // Map 1: {B -> 3}
        let mut map1 = BTreeMap::new();
        map1.insert(ch_b.clone(), (BigInt::from(3i64), mt));
        input.push(map1);

        // Map 2: {A -> 15, C -> 10}
        let mut map2 = BTreeMap::new();
        map2.insert(ch_a.clone(), (BigInt::from(15i64), mt));
        map2.insert(ch_c.clone(), (BigInt::from(10i64), mt));
        input.push(map2);

        // Calculate the differences
        let result =
            RholangMergingLogic::calculate_num_channel_diff(input, |arg0: &std::string::String| {
                get_data_on_hash(arg0.clone())
            });

        // Define expected results
        let mut expected = Vec::new();

        // Expected Map 0: {A -> 10}
        let mut expected_map0 = BTreeMap::new();
        expected_map0.insert(ch_a.clone(), (BigInt::from(10i64), mt));
        expected.push(expected_map0);

        // Expected Map 1: {B -> 3}
        let mut expected_map1 = BTreeMap::new();
        expected_map1.insert(ch_b.clone(), (BigInt::from(3i64), mt));
        expected.push(expected_map1);

        // Expected Map 2: {A -> -5, C -> -10}
        let mut expected_map2 = BTreeMap::new();
        expected_map2.insert(ch_a.clone(), (BigInt::from(-5i64), mt));
        expected_map2.insert(ch_c.clone(), (BigInt::from(-10i64), mt));
        expected.push(expected_map2);

        // Assert that the results match the expected values
        assert_eq!(result, expected);
    }

    #[test]
    fn test_calculate_num_channel_diff_bitmask() {
        // Verify bitmask diff semantics: diff = newly-set bits = end & !prev
        // Example: prev=0b0001, end=0b0101 → diff=0b0100 (bit 2 newly set)
        let ch = "X".to_string();
        let mt = MergeType::BitmaskOr;
        let mut init_values = HashMap::new();
        init_values.insert(ch.clone(), 0b0001i64);
        let get_initial =
            |k: &String| -> Option<BigInt> { init_values.get(k).copied().map(BigInt::from) };

        let mut map0 = BTreeMap::new();
        map0.insert(ch.clone(), (BigInt::from(0b0101i64), mt));
        let result = RholangMergingLogic::calculate_num_channel_diff(vec![map0], get_initial);

        let mut expected_map0 = BTreeMap::new();
        expected_map0.insert(ch.clone(), (BigInt::from(0b0100i64), mt));
        assert_eq!(result, vec![expected_map0]);
    }

    // ---- Phase-7 W7.1/W7.2: checked apply + checked diff (fail loudly, never
    // launder an overflowed/negative number-channel value into consensus state) ----

    fn test_hash() -> Blake2b256Hash { Blake2b256Hash::new(&[0u8; 4]) }

    fn test_rnd() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[0u8; 32]) }

    // A single-Par integer base datum holding `n`, produced through the exact
    // production encode path (create_datum_encoded) then decoded back, so the base
    // reader sees precisely what a real pre-state would.
    fn num_base_data(n: i64) -> Vec<Datum<ListParWithRandom>> {
        let encoded = RholangMergingLogic::create_datum_encoded(
            &test_hash(),
            &BigInt::from(n),
            MergeType::IntegerAdd,
            test_rnd(),
        )
        .unwrap();
        vec![serializers::decode_datum(&encoded)]
    }

    // One valid added-change entry so the RNG merge on an ACCEPTED path has input.
    fn one_change() -> ChannelChange<Vec<u8>> {
        let encoded = RholangMergingLogic::create_datum_encoded(
            &test_hash(),
            &BigInt::from(0),
            MergeType::IntegerAdd,
            test_rnd(),
        )
        .unwrap();
        ChannelChange {
            added: vec![encoded],
            removed: vec![],
        }
    }

    #[test]
    fn merge_integer_add_overflow_is_rejected() {
        // base = i64::MAX, diff = +1 -> checked_add overflows -> loud Err.
        // (The old wrapping_add silently committed i64::MIN to consensus state.)
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(1),
            MergeType::IntegerAdd,
            &ChannelChange::empty(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> {
                Ok(num_base_data(i64::MAX))
            },
        );
        assert!(matches!(res, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn merge_integer_add_negative_result_is_rejected() {
        // base = 0, diff = -1 -> checked_add = Some(-1) but < 0 -> Err. This is exactly
        // the negative vault balance the old code would have committed; at this site a
        // non-negative launder is structurally impossible for base >= 0, so the `>= 0`
        // rejection is the "never return a wrong Ok" guard.
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(-1),
            MergeType::IntegerAdd,
            &ChannelChange::empty(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> { Ok(vec![]) },
        );
        assert!(matches!(res, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn merge_integer_add_happy_path_writes_action() {
        // base = 10, diff = +5 -> 15, non-negative & in range -> Ok(write action).
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(5),
            MergeType::IntegerAdd,
            &one_change(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> { Ok(num_base_data(10)) },
        );
        assert!(matches!(
            res,
            Ok(HotStoreTrieAction::TrieInsertAction(
                TrieInsertAction::TrieInsertBinaryProduce(_)
            ))
        ));
    }

    #[test]
    fn merge_bitmask_or_unaffected_by_overflow_check() {
        // BitmaskOr never overflows; base = i64::MAX, diff = 1 -> Ok (no rejection).
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(1),
            MergeType::BitmaskOr,
            &one_change(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> {
                Ok(num_base_data(i64::MAX))
            },
        );
        assert!(res.is_ok());
    }

    #[test]
    fn diff_integer_add_recovers_wrapped_delta() {
        // A deploy whose EXECUTION overflowed at the language level (reduce.rs GInt `+`
        // wraps by design) stores a wrapped `end` value. wrapping_sub is the exact group
        // inverse of that wrapping add, so the diff recovers the deploy's TRUE intended
        // delta (here i64::MAX) even though `end` came back negative. The over-large delta
        // is then rejected DOWNSTREAM at combine (checked_add) / apply (Site 1) — NOT at
        // this diff step, which must succeed so the deploy can be gracefully merge-rejected.
        // This is why calculate_num_channel_diff must stay wrapping (see the Site 2 body).
        let ch = "X".to_string();
        let mt = MergeType::IntegerAdd;
        let prev = 10i64;
        let intended = i64::MAX;
        let wrapped_end = prev.wrapping_add(intended); // overflowed end, as execution stored it
        let mut init = HashMap::new();
        init.insert(ch.clone(), prev);
        let get_initial = |k: &String| -> Option<BigInt> { init.get(k).copied().map(BigInt::from) };

        let mut map0 = BTreeMap::new();
        map0.insert(ch.clone(), (BigInt::from(wrapped_end), mt));
        let result = RholangMergingLogic::calculate_num_channel_diff(vec![map0], get_initial);

        let mut expected = BTreeMap::new();
        expected.insert(ch.clone(), (BigInt::from(intended), mt)); // delta recovered exactly
        assert_eq!(result, vec![expected]);
    }
    fn bigint_base_data(n: &BigInt) -> Vec<Datum<ListParWithRandom>> {
        let encoded = RholangMergingLogic::create_datum_encoded(
            &test_hash(),
            n,
            MergeType::BigIntAdd,
            test_rnd(),
        )
        .unwrap();
        vec![serializers::decode_datum(&encoded)]
    }

    fn written_value(
        action: HotStoreTrieAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Par {
        match action {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(p)) => {
                let datum: Datum<ListParWithRandom> = serializers::decode_datum(&p.data[0]);
                datum.a.pars[0].clone()
            }
            _ => panic!("expected a binary produce insert"),
        }
    }

    #[test]
    fn merge_bigint_add_past_i64_writes_bigint_value() {
        let base = BigInt::from(i64::MAX);
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(i64::MAX),
            MergeType::BigIntAdd,
            &one_change(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> {
                Ok(bigint_base_data(&base))
            },
        )
        .expect("BigIntAdd must not overflow");
        assert_eq!(
            RhoBigInt::unapply(&written_value(res)),
            Some(BigInt::from(i64::MAX) * 2)
        );
    }

    #[test]
    fn merge_bigint_add_negative_result_is_rejected() {
        let res = RholangMergingLogic::calculate_number_channel_merge(
            &test_hash(),
            &BigInt::from(-11),
            MergeType::BigIntAdd,
            &one_change(),
            |_h| -> Result<Vec<Datum<ListParWithRandom>>, HistoryError> {
                Ok(bigint_base_data(&BigInt::from(10)))
            },
        );
        assert!(matches!(res, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn diff_bigint_add_is_exact() {
        let ch = "X".to_string();
        let prev = BigInt::from(i64::MAX);
        let end = BigInt::parse_bytes(b"1000000000000000000000000000000", 10).unwrap();
        let get_initial = |_k: &String| -> Option<BigInt> { Some(prev.clone()) };
        let mut map0 = BTreeMap::new();
        map0.insert(ch.clone(), (end.clone(), MergeType::BigIntAdd));
        let result = RholangMergingLogic::calculate_num_channel_diff(vec![map0], get_initial);
        let mut expected = BTreeMap::new();
        expected.insert(ch, (&end - &prev, MergeType::BigIntAdd));
        assert_eq!(result, vec![expected]);
    }

    #[test]
    fn number_merge_type_follows_value_kind() {
        let int_value = ListParWithRandom {
            pars: vec![RhoNumber::create_par(7)],
            random_state: vec![],
        };
        let bigint_value = ListParWithRandom {
            pars: vec![RhoBigInt::create_par(&BigInt::from(7))],
            random_state: vec![],
        };
        assert_eq!(
            RholangMergingLogic::number_merge_type(&int_value, MergeType::IntegerAdd),
            Some(MergeType::IntegerAdd)
        );
        assert_eq!(
            RholangMergingLogic::number_merge_type(&bigint_value, MergeType::IntegerAdd),
            Some(MergeType::BigIntAdd)
        );
        assert_eq!(
            RholangMergingLogic::number_merge_type(&int_value, MergeType::BitmaskOr),
            Some(MergeType::BitmaskOr)
        );
        assert_eq!(
            RholangMergingLogic::number_merge_type(&bigint_value, MergeType::BitmaskOr),
            None
        );
        assert_eq!(
            RholangMergingLogic::try_get_number_with_rnd(&bigint_value).map(|(n, _)| n),
            Some(BigInt::from(7))
        );
    }
}
