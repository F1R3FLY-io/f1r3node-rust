use std::collections::{BTreeSet, HashMap, HashSet, LinkedList};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use proptest::collection::vec;
use proptest::prelude::*;
use proptest_derive::Arbitrary;
use rand::prelude::SliceRandom;
use rand::thread_rng;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, HotStoreAction, InsertAction,
    InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rstest::*;
use serde::Serialize;

// See rspace/src/test/scala/coop/rchain/rspace/HotStoreSpec.scala

type Channel = String;
type Data = Datum<String>;
type Continuation = WaitingContinuation<Pattern, StringsCaptor>;
type Join = Vec<Channel>;
type Joins = Vec<Join>;

const SIZE_RANGE: usize = 10; // 10

proptest! {
  #![proptest_config(ProptestConfig {
    cases: 20, // 20
    failure_persistence: None,
    .. ProptestConfig::default()
})]

  // Double check these tests perform same logic as Scala tests for Joins
  // For example, 'arbitraryJoins' and double or nested vectors
  // What is difference between 'vec(any::<T>())' and 'any::<Vec<T>>()' in proptest?

  #[test]
  fn get_continuations_when_cache_is_empty_should_read_from_history_and_put_into_cache(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), history_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());

      let cache = hot_store.snapshot();
      assert!(cache.continuations.is_empty());

      let read_continuations = hot_store.get_continuations(&channels.clone());
      let cache = hot_store.snapshot();
      assert_eq!(cache.continuations.get(&channels).unwrap().clone(), history_continuations);
      assert_eq!(read_continuations, history_continuations);
  }

  #[test]
  fn get_continuations_when_cache_contains_data_should_read_from_cache_ignoring_history(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), history_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE), cached_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      let read_continuations = hot_store.get_continuations(&channels.clone());
      let cache = hot_store.snapshot();
      assert_eq!(cache.continuations.get(&channels).unwrap().clone(), cached_continuations);
      assert_eq!(read_continuations, cached_continuations);
  }

  #[test]
  fn get_continuations_should_include_installed_continuations(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), mut cached_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE), installed_continuation in any::<Continuation>()) {
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      hot_store.install_continuation(&channels.clone(), installed_continuation.clone());
      let res = hot_store.get_continuations(&channels);
      cached_continuations.insert(0, installed_continuation);
      assert_eq!(res, cached_continuations);
  }

  #[test]
  fn put_continuation_when_cache_is_empty_should_read_from_history_and_add_to_it(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), mut history_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE), inserted_continuation in any::<Continuation>()) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());
      hot_store.put_continuation(&channels.clone(), inserted_continuation.clone());

      let cache = hot_store.snapshot();
      history_continuations.insert(0, inserted_continuation);
      assert_eq!(cache.continuations.get(&channels).unwrap().clone(), history_continuations);
  }

  #[test]
  fn put_continuation_when_cache_contains_data_should_read_from_cache_and_add_to_it(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), history_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE), mut cached_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE), inserted_continuation in any::<Continuation>()) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      hot_store.put_continuation(&channels.clone(),inserted_continuation.clone());

      let cache = hot_store.snapshot();
      cached_continuations.insert(0, inserted_continuation);
      assert_eq!(cache.continuations.get(&channels).unwrap().clone(), cached_continuations);
  }

  #[test]
  fn install_continuation_should_cache_installed_continuations_separately(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE), mut cached_continuations
    in vec(any::<Continuation>(), 0..=SIZE_RANGE), inserted_continuation in any::<Continuation>(), installed_continuation in any::<Continuation>()) {
      prop_assume!(inserted_continuation != installed_continuation);
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      hot_store.install_continuation(&channels.clone(), installed_continuation.clone());
      hot_store.put_continuation(&channels.clone(), inserted_continuation.clone());

      let cache = hot_store.snapshot();
      cached_continuations.insert(0, inserted_continuation);
      assert_eq!(cache.installed_continuations.get(&channels).unwrap().clone(), installed_continuation);
      assert_eq!(cache.continuations.get(&channels).unwrap().clone(), cached_continuations);
  }

  #[test]
  fn remove_continuation_when_cache_is_empty_should_read_from_history_and_remove_the_continuation_from_loaded_data(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE),
    history_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE), index in any::<i32>()) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());
      let res = hot_store.remove_continuation(&channels.clone(), index);

      let state_lock = hot_store.snapshot();
      assert!(check_removal_works_or_fails_on_error(res, state_lock.continuations.get(&channels).map_or(Vec::new(), |x| x.clone()), history_continuations, index).is_ok());
  }

  #[test]
  fn remove_continuation_when_cache_contains_data_should_read_from_cache_and_remove_continuation_from_loaded_data(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE),
    history_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE), cached_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE), index in any::<i32>()) {
      let (history, hot_store) = fixture();

      history.put_continuations(channels.clone(), history_continuations.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      let res = hot_store.remove_continuation(&channels.clone(), index);
      let state_lock = hot_store.snapshot();
      assert!(check_removal_works_or_fails_on_error(res, state_lock.continuations.get(&channels).map_or(Vec::new(), |x| x.clone()), cached_continuations, index).is_ok());
  }

  #[test]
  fn remove_continuation_when_installed_continuation_is_present_should_not_allow_its_removal(channels in  vec(any::<Channel>(), 0..=SIZE_RANGE),
    mut cached_continuations in vec(any::<Continuation>(), 0..=SIZE_RANGE), installed_continuation in any::<Continuation>(), index in any::<i32>()) {
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), cached_continuations.clone())]), installed_continuations:  HashMap::from_iter(vec![(channels.clone(), installed_continuation.clone())]),
        data: HashMap::new(), joins: HashMap::new(), installed_joins: HashMap::new() });

      let res = hot_store.remove_continuation(&channels.clone(), index);
      if index == 0 {
        assert!(res.is_none());
      } else {
        // index of the removed continuation includes the installed
        let conts = hot_store.get_continuations(&channels);
        cached_continuations.insert(0, installed_continuation);
        assert!(check_removal_works_or_fails_on_error(res, conts, cached_continuations, index).is_ok());
      }
  }

  #[test]
  fn get_data_when_cache_is_empty_should_read_from_history_and_put_into_the_cache(channel in  any::<Channel>(), history_data in vec(any::<Datum<String>>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      let cache = hot_store.snapshot();
      assert!(cache.data.is_empty());

      let read_data = hot_store.get_data(&channel);
      let cache = hot_store.snapshot();
      assert_eq!(cache.data.get(&channel).unwrap().clone(), history_data);
      assert_eq!(read_data, history_data);
  }

  #[test]
  fn get_data_when_cache_contains_data_should_read_from_cache_ignoring_history(channel in  any::<Channel>(), history_data in vec(any::<Data>(), 0..=SIZE_RANGE),
    cached_data in vec(any::<Data>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::from_iter(vec![(channel.clone(), cached_data.clone())]), joins: HashMap::new(), installed_joins: HashMap::new() });

      let read_data = hot_store.get_data(&channel);
      let cache = hot_store.snapshot();
      assert_eq!(cache.data.get(&channel).unwrap().clone(), cached_data);
      assert_eq!(read_data, cached_data);
  }

  #[test]
  fn put_datum_when_cache_is_empty_should_read_from_history_and_add_to_it(channel in  any::<Channel>(), mut history_data in vec(any::<Data>(), 0..=SIZE_RANGE),
    inserted_data in any::<Data>()) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      hot_store.put_datum(&channel.clone(), inserted_data.clone());

      let cache = hot_store.snapshot();
      history_data.insert(0, inserted_data);
      assert_eq!(cache.data.get(&channel).unwrap().clone(), history_data);
  }

  #[test]
  fn put_datum_when_contains_data_should_read_from_cache_and_add_to_it(channel in  any::<Channel>(), history_data in vec(any::<Data>(), 0..=SIZE_RANGE),
    mut cached_data in vec(any::<Data>(), 0..=SIZE_RANGE), inserted_data in any::<Data>()) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::from_iter(vec![(channel.clone(), cached_data.clone())]), joins: HashMap::new(), installed_joins: HashMap::new() });

      hot_store.put_datum(&channel.clone(), inserted_data.clone());
      let cache = hot_store.snapshot();
      cached_data.insert(0, inserted_data);
      assert_eq!(cache.data.get(&channel).unwrap().clone(), cached_data);
  }

  #[test]
  fn remove_datum_when_cache_is_empty_should_read_from_history_and_remove_datum_at_index(channel in  any::<Channel>(), history_data in vec(any::<Data>(), 0..=SIZE_RANGE),
    index in any::<i32>()) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      let res = hot_store.remove_datum(&channel.clone(), index);

      let cache = hot_store.snapshot();
      assert!(check_datum_removal_works_or_fails_on_error(res, cache.data.get(&channel).map_or(Vec::new(), |x| x.clone()), history_data, index).is_ok());
  }

  #[test]
  fn remove_datum_when_cache_contains_data_should_read_from_cache_and_remove_datum(channel in  any::<Channel>(), history_data in vec(any::<Data>(), 0..=SIZE_RANGE),
    cached_data in vec(any::<Data>(), 0..=SIZE_RANGE), index in any::<i32>()) {
      let (history, hot_store) = fixture();

      history.put_data(channel.clone(), history_data.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::from_iter(vec![(channel.clone(), cached_data.clone())]), joins: HashMap::new(), installed_joins: HashMap::new() });

      let res = hot_store.remove_datum(&channel.clone(), index);
      let cache = hot_store.snapshot();
      assert!(check_datum_removal_works_or_fails_on_error(res, cache.data.get(&channel).unwrap().clone(), cached_data, index).is_ok());
  }

  #[test]
  fn get_joins_when_cache_is_empty_should_read_from_history_and_put_into_the_cache(channel in  any::<Channel>(), history_joins in vec(any::<Vec<Channel>>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      let cache = hot_store.snapshot();
      assert!(cache.joins.is_empty());

      let read_joins = hot_store.get_joins(&channel.clone());
      let cache = hot_store.snapshot();
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), history_joins);
      assert_eq!(read_joins, history_joins);
  }

  #[test]
  fn get_joins_when_cache_contains_data_should_read_from_cache_ignoring_history(channel in  any::<Channel>(), history_joins in vec(any::<Vec<Channel>>(), 0..=SIZE_RANGE),
    cached_joins in vec(any::<Vec<Channel>>(), 0..=SIZE_RANGE)) {
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      let read_joins = hot_store.get_joins(&channel.clone());
      let cache = hot_store.snapshot();
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
      assert_eq!(read_joins, cached_joins);
  }

  #[test]
  fn put_join_when_cache_is_empty_should_read_from_history_and_add_to_it(channel in  any::<Channel>(), mut history_joins in any::<Joins>(), inserted_join in any::<Join>()) {
      prop_assume!(!history_joins.contains(&inserted_join));
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      hot_store.put_join(&channel.clone(), &inserted_join.clone());

      let cache = hot_store.snapshot();
      history_joins.insert(0, inserted_join);
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), history_joins);
  }

  #[test]
  fn put_join_when_cache_contains_data_should_read_from_cache_and_add_to_it(channel in  any::<Channel>(), history_joins in any::<Joins>(), mut cached_joins in any::<Joins>(),
    inserted_join in any::<Join>()) {
      prop_assume!(!cached_joins.contains(&inserted_join));
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      hot_store.put_join(&channel.clone(), &inserted_join.clone());
      let cache = hot_store.snapshot();
      cached_joins.insert(0, inserted_join);
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
  }

  #[test]
  fn put_join_should_not_allow_inserting_duplicate_joins(channel in  any::<Channel>(), mut cached_joins in any::<Joins>(), inserted_join in any::<Join>()) {
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      hot_store.put_join(&channel.clone(), &inserted_join.clone());
      let cache = hot_store.snapshot();

      if !cached_joins.contains(&inserted_join) {
        cached_joins.insert(0, inserted_join);
        assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
      } else {
        assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
      }
  }

  #[test]
  fn install_join_should_cache_installed_joins_separately(channel in  any::<Channel>(), mut cached_joins in any::<Joins>(), inserted_join in any::<Join>(),
    installed_join in any::<Join>()) {
      prop_assume!(inserted_join != installed_join && !cached_joins.contains(&inserted_join));
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      hot_store.put_join(&channel.clone(), &inserted_join.clone());
      hot_store.install_join(&channel.clone(), &installed_join.clone());

      let cache = hot_store.snapshot();
      assert_eq!(cache.installed_joins.get(&channel).unwrap().clone(), vec![installed_join]);
      cached_joins.insert(0, inserted_join);
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
  }

  #[test]
  fn install_join_should_not_allow_installing_duplicate_joins_per_channel(channel in  any::<Channel>(), cached_joins in any::<Joins>(), installed_join in any::<Join>()) {
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      hot_store.install_join(&channel.clone(), &installed_join.clone());
      hot_store.install_join(&channel.clone(), &installed_join.clone());

      let cache = hot_store.snapshot();
      assert_eq!(cache.installed_joins.get(&channel).unwrap().clone(), vec![installed_join]);
  }

  #[test]
  fn remove_join_when_cache_is_empty_should_read_from_history_and_remove_join(channel in  any::<Channel>(), history_joins in any::<Joins>(), index in any::<i32>(), join in any::<Join>()) {
      prop_assume!(!history_joins.contains(&join));
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      let to_remove = history_joins.get(index as usize).unwrap_or(&join).clone();
      let res = hot_store.remove_join(&channel.clone(), &to_remove);

      let cache = hot_store.snapshot();
      assert!(check_removal_works_or_ignores_errors(res, cache.joins.get(&channel).unwrap().clone(), history_joins, index).is_ok());
  }

  #[test]
  fn remove_join_when_cache_contains_data_should_read_from_the_cache_and_remove_join(channel in  any::<Channel>(), history_joins in any::<Joins>(), cached_joins in any::<Joins>(),
    index in any::<i32>(), join in any::<Join>()) {
      prop_assume!(!cached_joins.contains(&join));
      let (history, hot_store) = fixture();

      history.put_joins(channel.clone(), history_joins.clone());
      let to_remove = cached_joins.get(index as usize).unwrap_or(&join).clone();
      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]), installed_joins: HashMap::new() });

      let res = hot_store.remove_join(&channel.clone(), &to_remove);
      let cache = hot_store.snapshot();
      assert!(check_removal_works_or_ignores_errors(res, cache.joins.get(&channel).unwrap().clone(), cached_joins, index).is_ok());
  }

  #[test]
  fn remove_join_when_installed_joins_are_present_should_not_allow_removing_them(channel in  any::<Channel>(), cached_joins in any::<Joins>(), installed_joins in any::<Joins>()) {
      prop_assume!(cached_joins != installed_joins && !installed_joins.is_empty());
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]),
        installed_joins: HashMap::from_iter(vec![(channel.clone(), installed_joins.clone())]) });

      let mut rng = thread_rng();
      let mut shuffled_joins = installed_joins.clone();
      shuffled_joins.shuffle(&mut rng);
      let to_remove = shuffled_joins.first().unwrap().clone();

      let res = hot_store.remove_join(&channel.clone(), &to_remove.clone());
      let cache = hot_store.snapshot();

      if !cached_joins.contains(&to_remove) {
        assert!(res.is_some());
        assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
      } else {
        let to_remove_count_in_cache = cache.joins.get(&channel).unwrap().clone().into_iter().filter(|x| x.clone() == to_remove).count();
        let to_remove_count_in_cached_joins = cached_joins.into_iter().filter(|x| x.clone() == to_remove).count();
        assert_eq!(to_remove_count_in_cache, to_remove_count_in_cached_joins - 1);
        assert_eq!(cache.installed_joins.get(&channel).unwrap().clone(), installed_joins);
      }
  }

  #[test]
  fn remove_join_should_not_remove_a_join_when_a_continuation_is_present(channel in  any::<Channel>(), continuation in any::<Continuation>(), cached_joins in any::<Joins>()) {
      prop_assume!(!cached_joins.is_empty());
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::new(), installed_continuations: HashMap::new(), data: HashMap::new(), joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]),
        installed_joins: HashMap::new() });

      let mut rng = thread_rng();
      let mut shuffled_joins = cached_joins.clone();
      shuffled_joins.shuffle(&mut rng);
      let to_remove = shuffled_joins.first().unwrap().clone();

      hot_store.put_continuation(&to_remove.clone(), continuation);
      let res = hot_store.remove_join(&channel.clone(), &to_remove.clone());
      let cache = hot_store.snapshot();

      assert!(res.is_some());
      assert_eq!(cache.joins.get(&channel).unwrap().clone(), cached_joins);
  }

    #[test]
  fn changes_should_return_information_to_be_persisted_in_history(channels in vec(any::<Channel>(), 0..=SIZE_RANGE), channel in  any::<Channel>(), continuations in  vec(any::<Continuation>(), 0..=SIZE_RANGE),
        installed_continuation in any::<Continuation>(), data in vec(any::<Data>(), 0..=SIZE_RANGE), joins in any::<Joins>()) {
      let (_, hot_store) = fixture();

      hot_store.set_state(HotStoreState { continuations: HashMap::from_iter(vec![(channels.clone(), continuations.clone())]), installed_continuations: HashMap::from_iter(vec![(channels.clone(), installed_continuation.clone())]),
                data: HashMap::from_iter(vec![(channel.clone(), data.clone())]), joins: HashMap::from_iter(vec![(channel.clone(), joins.clone())]),
        installed_joins: HashMap::new() });

            let res = hot_store.changes();
            let cache = hot_store.snapshot();
            assert_eq!(res.len(), cache.continuations.len() + cache.data.len() + cache.joins.len());

            if continuations.is_empty() {
        assert!(res.contains(&HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations { channels }))));
      } else {
        assert!(res.contains(&HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations { channels, continuations }))));
      }

      if data.is_empty() {
        assert!(res.contains(&HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel: channel.clone() }))));
      } else {
        assert!(res.contains(&HotStoreAction::Insert(InsertAction::InsertData(InsertData { channel: channel.clone(), data }))));
      }

      if joins.is_empty() {
        assert!(res.contains(&HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel }))));
      } else {
        assert!(res.contains(&HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins { channel, joins }))));
      }
  }

  #[test]
  fn concurrent_data_operations_on_disjoint_channels_should_not_mess_up_the_cache(channel1 in  any::<Channel>(), channel2 in  any::<Channel>(), mut history_data1 in vec(any::<Data>(), 0..=SIZE_RANGE), mut history_data2 in vec(any::<Data>(), 0..=SIZE_RANGE),
    inserted_data1 in any::<Data>(), inserted_data2 in any::<Data>()) {
      prop_assume!(channel1 != channel2);
      let (history, hot_store) = fixture();
      let hot_store = Arc::new(Mutex::new(hot_store));

      history.put_data(channel1.clone(), history_data1.clone());
      history.put_data(channel2.clone(), history_data2.clone());

      // Clone the Arc to share it between threads
     let hot_store1 = Arc::clone(&hot_store);
     let hot_store2 = Arc::clone(&hot_store);

     // Clone the channels and data to move them into the threads
     let channel1_clone = channel1.clone();
     let channel2_clone = channel2.clone();
     let inserted_data1_clone = inserted_data1.clone();
     let inserted_data2_clone = inserted_data2.clone();

      // Spawn two threads to run put_datum in parallel and waits for both threads to complete using join.
      let handle1 = std::thread::spawn(move || {
        let hot_store = hot_store1.lock().unwrap();
        hot_store.put_datum(&channel1_clone, inserted_data1_clone);
      });
      let handle2 = std::thread::spawn(move || {
        let hot_store = hot_store2.lock().unwrap();
        hot_store.put_datum(&channel2_clone, inserted_data2_clone);
      });
      handle1.join().unwrap();
      handle2.join().unwrap();

      let r1 = hot_store.lock().unwrap().get_data(&channel1);
      let r2 = hot_store.lock().unwrap().get_data(&channel2);
      history_data1.insert(0, inserted_data1);
      history_data2.insert(0, inserted_data2);

      assert_eq!(r1, history_data1);
      assert_eq!(r2, history_data2);
  }

  #[test]
  fn concurrent_coninuation_operations_on_disjoint_channels_should_not_mess_up_the_cache(channels1 in  vec(any::<Channel>(), 0..=SIZE_RANGE), channels2 in  vec(any::<Channel>(), 0..=SIZE_RANGE), mut history_continuations1 in vec(any::<Continuation>(), 0..=SIZE_RANGE),
    mut history_continuations2 in vec(any::<Continuation>(), 0..=SIZE_RANGE), inserted_continuation1 in any::<Continuation>(), inserted_continuation2 in any::<Continuation>()) {
      prop_assume!(channels1 != channels2);
      let (history, hot_store) = fixture();
      let hot_store = Arc::new(Mutex::new(hot_store));

      history.put_continuations(channels1.clone(), history_continuations1.clone());
      history.put_continuations(channels2.clone(), history_continuations2.clone());

      // Clone the Arc to share it between threads
     let hot_store1 = Arc::clone(&hot_store);
     let hot_store2 = Arc::clone(&hot_store);

     // Clone the channels and data to move them into the threads
     let channels1_clone = channels1.clone();
     let channels2_clone = channels2.clone();
     let inserted_continuation1_clone = inserted_continuation1.clone();
     let inserted_continuation2_clone = inserted_continuation2.clone();

      // Spawn two threads to run put_datum in parallel and waits for both threads to complete using join.
      let handle1 = std::thread::spawn(move || {
        let hot_store = hot_store1.lock().unwrap();
        hot_store.put_continuation(&channels1_clone, inserted_continuation1_clone);
      });
      let handle2 = std::thread::spawn(move || {
        let hot_store = hot_store2.lock().unwrap();
        hot_store.put_continuation(&channels2_clone, inserted_continuation2_clone);
      });
      handle1.join().unwrap();
      handle2.join().unwrap();

      let r1 = hot_store.lock().unwrap().get_continuations(&channels1);
      let r2 = hot_store.lock().unwrap().get_continuations(&channels2);
      history_continuations1.insert(0, inserted_continuation1);
      history_continuations2.insert(0, inserted_continuation2);

      assert_eq!(r1, history_continuations1);
      assert_eq!(r2, history_continuations2);
  }

  /* BREAK */

  #[test]
  fn put_datum_should_put_datum_in_a_new_channel(channel in  any::<String>(), datum_value in any::<String>()) {
      let (_, hot_store) = fixture();
      let key = channel.clone();
      let datum = Datum::create(&channel, datum_value, false);

      hot_store.put_datum(&key.clone(), datum.clone());
      let res = hot_store.get_data(&key);
      assert!(check_same_elements(res, vec![datum]));
  }

  #[test]
  fn put_datum_should_append_datum_if_channel_already_exists(channel in  any::<String>(), datum_value in any::<String>()) {
      let (_, hot_store) = fixture();
      let key = channel.clone();
      let datum1 = Datum::create(&channel, datum_value.clone(), false);
      let datum2 = Datum::create(&channel, datum_value + "2", false);

      hot_store.put_datum(&key.clone(), datum1.clone());
      hot_store.put_datum(&key.clone(), datum2.clone());
      let res = hot_store.get_data(&key);
      assert!(check_same_elements(res, vec![datum1, datum2]));
  }

  // TODO: Set min_successful to 10
  // TODO: Double chck test case matches because of validIndices on Scala side
  #[test]
  fn remove_datum_should_remove_datum_at_index(channel in  any::<String>(), datum_value in any::<String>(), index in any::<i32>()) {
      let (_, hot_store) = fixture();
      let key = channel.clone();
      let data: Vec<Datum<String>> = (0..11)
        .map(|i| Datum::create(&channel, datum_value.clone() + &i.to_string(), false))
        .collect();

      for d in data.clone() {
        hot_store.put_datum(&key.clone(), d);
      }

      let _ = hot_store.remove_datum(&key.clone(), index - 1);
      let res = hot_store.get_data(&key);
      let expected: Vec<Datum<String>> = data.into_iter()
         .filter(|d| *d.a != datum_value.clone() + &(11 - index).to_string())
         .collect();
      assert!(check_same_elements(res, expected));
  }

  #[test]
  fn put_waiting_continuation_should_put_waiting_continuation_in_a_new_channel(channel in  any::<String>(), pattern in any::<String>()) {
      let (_, hot_store) = fixture();
      let key = vec![channel.clone()];
      let patterns = vec![Pattern::StringMatch(pattern)];
      let continuation = StringsCaptor::new();
      let wc = WaitingContinuation::create(&key, &patterns, &continuation, false, BTreeSet::default());

      hot_store.put_continuation(&key.clone(), wc.clone());
      let res = hot_store.get_continuations(&key);
      assert_eq!(res, vec![wc]);
  }

  #[test]
  fn put_waiting_continuation_should_append_continuation_if_channel_already_exists(channel in  any::<String>(), pattern in any::<String>()) {
      let (_, hot_store) = fixture();
      let key = vec![channel.clone()];
      let patterns = vec![Pattern::StringMatch(pattern.clone())];
      let continuation = StringsCaptor::new();
      let wc1 = WaitingContinuation::create(&key, &patterns, &continuation, false, BTreeSet::default());
      let wc2 = WaitingContinuation::create(&key, &vec![Pattern::StringMatch(pattern + "2")], &continuation, false, BTreeSet::default());

      hot_store.put_continuation(&key.clone(), wc1.clone());
      hot_store.put_continuation(&key.clone(), wc2.clone());
      let res = hot_store.get_continuations(&key);
      assert!(check_same_elements(res, vec![wc1, wc2]));
  }

  #[test]
  fn remove_waiting_continuation_should_remove_waiting_continuation_from_index(channel in  any::<String>(), pattern in any::<String>()) {
      let (_, hot_store) = fixture();
      let key = vec![channel.clone()];
      let patterns = vec![Pattern::StringMatch(pattern.clone())];
      let continuation = StringsCaptor::new();
      let wc1 = WaitingContinuation::create(&key, &patterns, &continuation, false, BTreeSet::default());
      let wc2 = WaitingContinuation::create(&key, &vec![Pattern::StringMatch(pattern + "2")], &continuation, false, BTreeSet::default());

      hot_store.put_continuation(&key.clone(), wc1.clone());
      hot_store.put_continuation(&key.clone(), wc2.clone());
      hot_store.remove_continuation(&key.clone(), 0);
      let res = hot_store.get_continuations(&key);
      assert!(check_same_elements(res, vec![wc1]));
  }

  #[test]
  fn add_join_should_add_join_for_a_channel(channel in  any::<String>(), channels in vec(any::<String>(), 0..=SIZE_RANGE)) {
      let (_, hot_store) = fixture();

      hot_store.put_join(&channel.clone(), &channels.clone());
      let res = hot_store.get_joins(&channel);
      assert_eq!(res, vec![channels]);
  }

  #[test]
  fn remove_join_should_remove_join_for_a_channel(channel in  any::<String>(), channels in vec(any::<String>(), 0..=SIZE_RANGE)) {
      let (_, hot_store) = fixture();

      hot_store.put_join(&channel.clone(), &channels.clone());
      hot_store.remove_join(&channel.clone(), &channels.clone());
      let res = hot_store.get_joins(&channel);
      assert!(res.is_empty());
  }

  #[test]
  fn remove_join_should_remove_only_passed_in_joins_for_a_channel(channel in  any::<String>(), channels in vec(any::<String>(), 0..=SIZE_RANGE)) {
      let (_, hot_store) = fixture();

      hot_store.put_join(&channel.clone(), &channels.clone());
      hot_store.put_join(&channel.clone(), &vec!["other_channel".to_string()]);
      hot_store.remove_join(&channel.clone(), &channels.clone());
      let res = hot_store.get_joins(&channel);
      assert_eq!(res, vec![vec!["other_channel".to_string()]]);
  }

  #[test]
  fn snapshot_should_create_a_copy_of_the_cache(_cache in  any::<String>()) {
      let channels = vec!["ch1".to_string(), "ch2".to_string()];
      let channel = channels[0].clone();
      let continuation = WaitingContinuation::<Pattern, StringsCaptor>::default();
      let cache = HotStoreState {
          continuations: HashMap::from_iter(vec![(channels.clone(), vec![continuation.clone()])]),
          installed_continuations: HashMap::from_iter(vec![(channels.clone(), continuation)]),
          data: HashMap::from_iter(vec![(channel.clone(), vec![Datum::<String>::default()])]),
          joins: HashMap::from_iter(vec![(channel.clone(), vec![channels.clone()])]),
          installed_joins: HashMap::from_iter(vec![(channel, vec![channels.clone()])]),
      };
      let (_, hot_store) = fixture_with_cache(cache.clone());

      let snapshot = hot_store.snapshot();
      assert!(compare_hashmaps(&snapshot.continuations, &cache.continuations));
      assert!(compare_hashmaps(&snapshot.installed_continuations, &cache.installed_continuations));
      assert!(compare_hashmaps(&snapshot.data, &cache.data));
      assert!(compare_hashmaps(&snapshot.joins, &cache.joins));
      assert!(compare_hashmaps(&snapshot.installed_joins, &cache.installed_joins));
  }

  #[test]
  fn remove_join_should_create_a_deep_copy_of_the_continuations_in_the_cache(channels in  vec(any::<String>(), 0..=SIZE_RANGE), continuation1 in any::<Continuation>(), continuation2 in any::<Continuation>()) {
      prop_assume!(continuation1 != continuation2);
      let (_, hot_store) = fixture();

      hot_store.put_continuation(&channels.clone(), continuation1.clone());
      let snapshot = hot_store.snapshot();
      hot_store.put_continuation(&channels.clone(), continuation2.clone());
      assert!(snapshot.continuations.get(&channels).unwrap().clone().contains(&continuation1));
      assert!(!snapshot.continuations.get(&channels).unwrap().clone().contains(&continuation2));
  }

  #[test]
  fn remove_join_should_create_a_deep_copy_of_the_installed_continuations_in_the_cache(channels in  vec(any::<String>(), 0..=SIZE_RANGE), continuation1 in any::<Continuation>(), continuation2 in any::<Continuation>()) {
      prop_assume!(continuation1 != continuation2);
      let (_, hot_store) = fixture();

      hot_store.install_continuation(&channels.clone(), continuation1.clone());
      let snapshot = hot_store.snapshot();
      hot_store.install_continuation(&channels.clone(), continuation2.clone());
      assert_eq!(snapshot.installed_continuations.get(&channels).unwrap().clone(), continuation1);
  }

  #[test]
  fn remove_join_should_create_a_deep_copy_of_the_data_in_the_cache(channel in  any::<String>(), data1 in any::<Data>(), data2 in any::<Data>()) {
      prop_assume!(data1 != data2);
      let (_, hot_store) = fixture();

      hot_store.put_datum(&channel.clone(), data1.clone());
      let snapshot = hot_store.snapshot();
      hot_store.put_datum(&channel.clone(), data2.clone());
      assert!(!snapshot.data.get(&channel).unwrap().clone().contains(&data2));
  }

  #[test]
  fn remove_join_should_create_a_deep_copy_of_the_joins_in_the_cache(channel in  any::<String>(), join1 in any::<Join>(), join2 in any::<Join>()) {
      prop_assume!(join1 != join2);
      let (_, hot_store) = fixture();

      hot_store.put_join(&channel.clone(), &join1.clone());
      let snapshot = hot_store.snapshot();
      hot_store.put_join(&channel.clone(), &join2.clone());
      assert!(!snapshot.joins.get(&channel).unwrap().clone().contains(&join2));
  }

  #[test]
  fn remove_join_should_create_a_deep_copy_of_the_installed_joins_in_the_cache(channel in  any::<String>(), join1 in any::<Join>(), join2 in any::<Join>()) {
      prop_assume!(join1 != join2);
      let (_, hot_store) = fixture();

      hot_store.install_join(&channel.clone(), &join1.clone());
      let snapshot = hot_store.snapshot();
      hot_store.install_join(&channel.clone(), &join2.clone());
      assert!(snapshot.installed_joins.get(&channel).unwrap().clone().contains(&join1));
      assert!(!snapshot.installed_joins.get(&channel).unwrap().clone().contains(&join2));
  }
}

#[test]
fn to_map_returns_the_union_of_data_and_continuation_key_domains() {
    let (_, hot_store) = fixture();
    let data_channel = "data-only".to_string();
    let waiting_channels = vec!["waiting-only".to_string()];
    let joined_channels = vec!["left".to_string(), "right".to_string()];
    let datum = Datum::<String>::default();
    let continuation = Continuation::default();

    hot_store.put_datum(&data_channel, datum.clone());
    hot_store.put_continuation(&waiting_channels, continuation.clone());
    hot_store.put_continuation(&joined_channels, continuation.clone());
    hot_store.install_continuation(&joined_channels, continuation);

    let mapped = hot_store.to_map();
    assert_eq!(mapped.len(), 3, "one row per distinct key domain member");

    let data_row = mapped
        .get(std::slice::from_ref(&data_channel))
        .expect("a data-only channel must be present");
    assert_eq!(data_row.data, vec![datum]);
    assert!(data_row.wks.is_empty());

    let waiting_row = mapped
        .get(&waiting_channels)
        .expect("a continuation-only channel must be present");
    assert!(waiting_row.data.is_empty());
    assert_eq!(waiting_row.wks.len(), 1);

    let joined_row = mapped
        .get(&joined_channels)
        .expect("a multi-channel continuation key must be present");
    assert!(joined_row.data.is_empty());
    assert_eq!(
        joined_row.wks.len(),
        2,
        "ordinary and installed continuations at the same join key must merge"
    );
}

fn check_removal_works_or_fails_on_error<T>(
    res: Option<()>,
    actual: Vec<T>,
    initial: Vec<T>,
    index: i32,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: PartialEq + Debug + Clone,
{
    if index < 0 || index >= initial.len().try_into().unwrap() {
        assert!(res.is_none());
        assert_eq!(actual, initial);
    } else {
        assert!(res.is_some());
        let expected: Vec<T> = initial
            .iter()
            .enumerate()
            .filter(|&(i, _)| i as i32 != index)
            .map(|(_, item)| item.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    Ok(())
}

fn check_datum_removal_works_or_fails_on_error<T>(
    res: Result<(), rspace_plus_plus::rspace::errors::RSpaceError>,
    actual: Vec<T>,
    initial: Vec<T>,
    index: i32,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: PartialEq + Debug + Clone,
{
    if index < 0 || index >= initial.len().try_into().unwrap() {
        assert!(res.is_err());
        assert_eq!(actual, initial);
    } else {
        assert!(res.is_ok());
        let expected: Vec<T> = initial
            .iter()
            .enumerate()
            .filter(|&(i, _)| i as i32 != index)
            .map(|(_, item)| item.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    Ok(())
}

fn check_removal_works_or_ignores_errors<T>(
    res: Option<()>,
    actual: Vec<T>,
    initial: Vec<T>,
    index: i32,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: PartialEq + Debug + Clone,
{
    if index < 0 || index >= initial.len().try_into().unwrap() {
        assert!(res.is_some());
        assert_eq!(actual, initial);
    } else {
        assert!(res.is_some());
        let expected: Vec<T> = initial
            .iter()
            .enumerate()
            .filter(|&(i, _)| i as i32 != index)
            .map(|(_, item)| item.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    Ok(())
}

// We only care that both vectors contain the same elements, not their ordering
fn check_same_elements<T: Hash + Eq>(vec1: Vec<T>, vec2: Vec<T>) -> bool {
    let set1: HashSet<_> = vec1.into_iter().collect();
    let set2: HashSet<_> = vec2.into_iter().collect();
    set1 == set2
}

pub fn compare_hashmaps<K, V>(map1: &HashMap<K, V>, map2: &HashMap<K, V>) -> bool
where
    K: Eq + Hash,
    V: PartialEq,
{
    map1 == map2
}

// See rspace/src/main/scala/coop/rchain/rspace/examples/StringExamples.scala
#[derive(Clone, Debug, Default, PartialEq, Arbitrary, Serialize, Eq, Hash)]
pub enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}
// Default-body event-hash bytes for the test-local type.
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

#[derive(Clone, Debug, Default, PartialEq, Arbitrary, Serialize, Eq, Hash)]
pub struct StringsCaptor {
    res: LinkedList<Vec<String>>,
}
// Default-body event-hash bytes for the test-local type.
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize
    for StringsCaptor
{
}

impl StringsCaptor {
    fn new() -> Self {
        StringsCaptor {
            res: LinkedList::new(),
        }
    }
}

// See rspace/src/test/scala/coop/rchain/rspace/HotStoreSpec.scala
#[derive(Clone)]
pub struct TestHistory<C: Eq + Hash, P: Clone, A: Clone, K: Clone> {
    state: Arc<Mutex<HotStoreState<C, P, A, K>>>,
}

impl<
    C: Clone + Eq + Hash + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
> HistoryReaderBase<C, P, A, K> for TestHistory<C, P, A, K>
{
    fn get_data(&self, channel: &C) -> Vec<Datum<A>> {
        let state_lock = self.state.lock().unwrap();
        let data = state_lock
            .data
            .get(channel)
            .map(|v| v.to_vec())
            .unwrap_or_else(|| Vec::new());
        data
    }

    fn get_continuations(&self, channels: &Vec<C>) -> Vec<WaitingContinuation<P, K>> {
        let state_lock = self.state.lock().unwrap();
        let continuations = state_lock
            .continuations
            .get(channels)
            .map(|v| v.to_vec())
            .unwrap_or_else(|| Vec::new());
        continuations
    }

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>> {
        let state_lock = self.state.lock().unwrap();
        let joins = state_lock
            .joins
            .get(channel)
            .map(|v| v.to_vec())
            .unwrap_or_else(|| Vec::new());
        joins
    }

    fn get_data_proj(&self, _key: &C) -> Vec<Datum<A>> { todo!() }

    fn get_continuations_proj(&self, _key: &Vec<C>) -> Vec<WaitingContinuation<P, K>> { todo!() }

    fn get_joins_proj(&self, _key: &C) -> Vec<Vec<C>> { todo!() }
}

impl<C: Eq + Hash, P: Clone, A: Clone, K: Clone> TestHistory<C, P, A, K> {
    fn put_data(&self, channel: C, data: Vec<Datum<A>>) -> () {
        let mut state = self.state.lock().unwrap();
        state.data.insert(channel, data);
    }

    fn put_continuations(
        &self,
        channels: Vec<C>,
        continuations: Vec<WaitingContinuation<P, K>>,
    ) -> () {
        let mut state = self.state.lock().unwrap();
        state.continuations.insert(channels, continuations);
    }

    fn put_joins(&self, channel: C, joins: Vec<Vec<C>>) -> () {
        let mut state = self.state.lock().unwrap();
        state.joins.insert(channel, joins);
    }
}

type StateSetup = (
    TestHistory<String, Pattern, String, StringsCaptor>,
    Box<dyn HotStore<String, Pattern, String, StringsCaptor>>,
);

#[fixture]
pub fn fixture() -> StateSetup {
    let history_state =
        Arc::new(Mutex::new(HotStoreState::<String, Pattern, String, StringsCaptor>::default()));

    let history = TestHistory {
        state: history_state.clone(),
    };

    let hot_store = HotStoreInstances::create_from_hs_and_hr(
        HotStoreState::default(),
        Box::new(history.clone()),
    );
    (history, hot_store)
}

pub fn fixture_with_cache(
    cache: HotStoreState<String, Pattern, String, StringsCaptor>,
) -> StateSetup {
    let history_state =
        Arc::new(Mutex::new(HotStoreState::<String, Pattern, String, StringsCaptor>::default()));

    let history = TestHistory {
        state: history_state.clone(),
    };

    let hot_store = HotStoreInstances::create_from_hs_and_hr(cache, Box::new(history.clone()));
    (history, hot_store)
}

// ── the recorded counterexample, PROMOTED
// ───────────────────────────────────────────
//
// The single entry of `rspace++/proptest-regressions/hot_store_spec.txt`,
// written out as a named test.
//
// ★ THE LITERALS WERE GENERATED, NOT TRANSCRIBED, AND FULLY ESCAPED. The entry
// is 18 KB of adversarial Unicode across 9 join groups (0, 69, 72, 92, 11, 94,
// 61, 6 and 47 strings). It was decoded from the corpus by a depth- and
// string-aware scanner and re-emitted with EVERY non-ASCII scalar as `\u{..}`.
// Carrying the characters through raw looked simpler and was wrong: a raw
// astral-plane scalar sitting next to a backslash escape is ambiguous
// on the round trip, and the first attempt produced a value whose `Debug`
// differed from the corpus at exactly one character out of 13,696 — caught by
// the assertion below, which is the entire reason it is written against the
// corpus FILE instead of a pasted copy.

/// The four recorded bindings of
/// `cc 9209dda1ef18f70480507bce69ed4f2957bbad8352216ad7f2f361fa36a94ede`.
// Nightly rustfmt's `format_strings` reflow can split the generated `\u{...}` escapes below,
// changing the corpus and even producing invalid Rust. Keep this generated oracle byte-stable.
#[rustfmt::skip]
fn recorded_case() -> (String, Vec<Vec<String>>, Vec<String>, Vec<String>) {
    let channel: String = "".to_string();
    let cached_joins: Vec<Vec<String>> = vec![vec![], vec!["\u{1083c}\u{c5}&*\u{d2}\u{18d05}\u{16a74}:\u{10b4b}c\u{100a8}<\u{23a}\u{10387}\u{1f574}\u{1d192}=\u{11341}:\u{1ee24}\u{10cb0}\u{1f03f}\u{23a}".to_string(), "\u{11340}\u{dca}\u{fffd}T$i&\u{9ce}\u{165e}{\u{1bc68}\u{ebc}(u`\u{1f574}\u{1f0bc}:\u{fb14}e".to_string(), "%\u{11f10}".to_string(), "\u{3e0}\u{e95}&\\".to_string(), "\u{a5}8\u{b07}\u{1a15}\u{fa}!ol\u{16b5f}F\u{1f29}$\u{1affe}:\u{1ee59}\u{12ae}\u{1f5d}g\u{10594}/\u{a5}\u{468}\u{1d4aa}:".to_string(), "=t".to_string(), "\u{a5}\u{831}\u{1cf08}%\u{b8}NY%/\u{aa1}\u{a9d5}O&\u{b2e}:\u{1f574}\u{fffd}".to_string(), "\u{1da9d}\u{fffd}R".to_string(), "{r\u{dc2}".to_string(), "hV&&\u{1f417}!(^\u{16e0}\\*\u{a5}&\u{1773}\u{110e1}\u{1f49}\u{cd6}\u{fffd}\"\"=\u{aa51}U$*&.".to_string(), "GY\u{ff}\u{aaae}\u{1f0a8}\u{23a}\u{1e8d2}o-q\u{af1})\u{c0c}\u{11288}W\u{1e2ff}\u{23a}<\u{11036}\\:Cc$'`\u{1937}".to_string(), "\u{20f0}<\u{ac2}\u{b77}&dc{\u{ebd}\u{a39}\u{23a}\u{1f7f0}\u{12473}.\"\u{238f}?\u{10a05}\u{a5e}\u{468}\u{1bc9e}/\"".to_string(), "\u{ab0d}\u{468}I{".to_string(), "\u{468}\u{176e}V\u{10559}\u{ea5}\u{1eea9}\u{16f42}`d\u{1f574}e".to_string(), "\u{b60}*\"'\u{119cf}3z0".to_string(), "V:\u{10c7}\u{fffd}\u{aae0}\u{fffd}\u{17084}eB.g'%\u{1999}Y;a\u{1315}&".to_string(), "\u{a832}H\u{11d23}\u{d83}<\u{fffd}\u{12c0}/cZ<=\"k\u{2dce}.\u{3119}{/\u{1ee51}\u{d5}\\\u{119a4}\u{fc42}\u{1a60}\\\u{1affe}j\u{104bb}".to_string(), "\u{fffd} \u{cef}%Q\u{dca}A61".to_string(), "o`\u{1709}".to_string(), "%\u{8aa}%\u{2d27}|\u{ffb4}\u{fe48}+\u{a5}\u{b03}\u{10102}a\u{23a}k^/\\9U-&H?\u{b9}\u{fbff}\u{145a2}q\u{2dde}\u{1ee37}\u{468}\u{1ee51}&".to_string(), "'\u{20b1}\u{10b8b}\u{10b60}\u{1eeb8}&={O*\u{1f0c9}".to_string(), "S\u{468}P\u{a68}:i\u{11063}\u{dc6}{^\\\u{468}.\u{114a0}f\u{2ee6}%\"^\u{164f}`\u{c8f}".to_string(), "?\u{fec7}\u{9f2}o".to_string(), "'CdlEk\u{17d15}%Z\u{dd}\u{1f574}".to_string(), "((\\%\u{ab9}H<\u{1bc7a}$?\u{104a1}${\u{1d464}*$\u{c2d}`:\u{1e8c2}I*\u{1faf1}\\\u{11da8}\u{ab4f}[\u{b39}?".to_string(), "\"<\u{1bc91}k\u{1036a}&\"\u{23a}\u{1f574}\u{a8c}".to_string(), "c\u{1ee81}:\u{468}=\u{a881}{O\u{1f011}\u{23a}\u{468}\\?U\u{fffd}*$\u{102f3}/\u{ce8}{\u{a5}P\u{468}\u{fc}T/E".to_string(), "\\\u{c46}`'\u{11a3d}\u{cde}\u{1fd9}%*y\u{1b006}\u{bd}:Qe\u{1082c}\u{468}%".to_string(), ":$$".to_string(), "g\u{1f45}\u{a5}\u{3190}\u{1f574}$:\u{10102}\u{1d105}\"%$<\u{214c}\u{c99}4\u{fb15},\u{fffd}\u{a8a0}F!/".to_string(), "'\u{104a6}/{\u{11f0d}+W<\u{468}_&\u{468}s\u{1ecb3}:\u{a5}&\\*".to_string(), "\u{ce2}nE\u{a7d3}\\\u{c48}\u{1135e}\u{10f0d}{".to_string(), "*\u{fffd}\u{1e7e3}\u{23a}\u{112f0}\u{a5}!\u{1f574}%=\u{2098}&\u{118ae}r:\u{12c0}3\u{10e9f}*`\u{1f574}$*:".to_string(), "$;&\u{aa50}\"\u{1abe}\u{f5}\u{107b9}\u{1d54b}\u{1e024}\u{a5}x%\u{13d1}\u{115b3}\\9\u{1099a}/\u{1ee52}:v\u{a5}\u{108f4}*\u{468}\u{1c43}".to_string(), "\u{a5}\u{1d90f}`$\u{1ee16}Y\"%\u{111eb}=\u{a5}\u{b48}\u{1191d} \\_<w\u{396}".to_string(), "\"\u{1a93}<$".to_string(), "\u{2cfc}\u{1ee7e}\u{1fa83}\u{1f995}9{4\u{1f574}\u{1f006}\u{1086e}':1|".to_string(), "=w\u{9e0}\u{839}{:\u{10af0}\\=\u{23a}".to_string(), "\u{1036e}\u{468}*'?*\"d'<\u{1f574}6\u{d2}\u{b99}\u{1ee1f}\u{1ee62}O\u{1130f}\u{23a}+\u{fffd}.\u{e33}=".to_string(), "\u{1cf0e}".to_string(), "\u{e5}\"\u{468}\u{fffd}".to_string(), "9".to_string(), "\u{a5}%\u{1f574}\\/\u{a36}\"\u{fa9f}<h(da".to_string(), "\u{ba4}$m\u{cb6}&?\u{23a}\"{:\u{b33}$\u{23a}\u{468}@W\u{10583}=`\u{16b58}'&".to_string(), "\u{1134d}*\u{1f71b}MEw\u{ffdb}\u{10054}WR\u{1ee68}B\"".to_string(), "\u{1e7e5}<".to_string(), "7;[]#{\u{11d67}\u{af9}B\u{c5d}\u{149}8\u{10534};\u{1892}%'5".to_string(), "\u{9dc}u{\u{1d378}i".to_string(), "T\u{11f06}%#\u{5e0}=\"\u{9bc}FGA'\u{10e96}{\u{aa5}".to_string(), "\u{23a}=`".to_string(), "{ \u{468}\u{b7}?//\u{ae0}\u{1ee70}1B=F.\u{1940}S\u{b2}'t\u{dc1}\u{105b7}\u{b0}".to_string(), "\u{2094}".to_string(), "&i\u{fb8a}oN\"\\".to_string(), ":\u{468}\\\u{1119e}\"m}'.d\u{1f858}#\u{a90e}?\u{10558}d\u{2e4dd}\u{109a5}=\u{10016}\u{a861}\u{865}\\\u{757}\u{468}".to_string(), "q\u{16b5f}\u{31af}q\u{12fef}/z\u{468}t\u{1128b}:*\"'ad*G*\u{10a50}\u{1d425}S:`".to_string(), "\"\u{fa48}`?} \u{1ee59}\u{1f574}<\"b\u{a5}%*r`\u{bc0}\u{1daa7}\u{fe}\u{1ee47}\u{2de8}\u{fffd}\u{ce0}\u{116c5}\u{468}?:".to_string(), "Oa\u{16b53}.nA\u{10b34}F\u{a9fb}\u{1f574}\u{1fdb}c\u{de}n\"`\u{17e7}=Z\u{fd98}".to_string(), "\u{2090}7\u{a32}@\u{d9}+$[b\u{1a50}\u{1f574}`\u{468}\u{1d415}v\u{ccb}\u{aa96}=m`e\u{a5}<.\u{fffd}\u{10d31}".to_string(), "\u{d2}Q\u{11338}\u{1191d}\u{11668}\u{a5}\u{a6}\u{1b7c}=\u{11347}".to_string(), ".XU\u{103c9}=\\\u{a5}\u{1fe3}\u{1d1e1}?g\u{1008b}=\u{104a2}\u{311c}t\u{a5}/\u{d0e}=`".to_string(), "Q\u{1346}aPU\u{ea3}".to_string(), "\u{1f305}X\u{1f0c3}\u{dd9}'\u{10563}\u{1235b}:=.nu:1<\u{1f574}6C~\u{468}\u{28a})\u{1bc73}\u{1329}<o".to_string(), "?$\\f\u{23a}1\u{10a41}\u{fffd}/\u{ae2}\u{5ef}*\u{23a}E:\u{1e023}\u{b4c}/w<$\u{9b2}PW\u{dca}Y\u{b02}\u{a1f}\u{b6a}3\u{fb3e}$".to_string(), "\u{a84f}\u{1ee7a}Dn<x\u{11d3a}\u{6f2}".to_string(), "\u{a5}$6{X..\u{1f20}q\u{1f574}${*\u{23a}\u{1d628}\u{10b81},\u{a682}\u{23a}\u{101ec}<E\u{b10}\".n=\u{110f6}\"\u{1c99}\u{ac}\u{1e001}".to_string(), "\"=\u{ab02}:\u{11373}&\u{23a}\u{1f574}]\u{114d7}".to_string(), "\\{\u{1ee36}`".to_string(), "\u{136a}\u{1f94d}\"\u{c55}E\u{16b5f}A\u{1cc2}u\u{fffd}.:\u{2090}\"*/%3\u{468}'\u{1ba1}\u{b5c}\u{2dba}".to_string(), "".to_string()], vec!["6\u{dd4}\u{ba4}\u{1926}\u{1f574}\u{1fa76}\u{cee}Cf:Y\u{12441}<=55\u{11620}O0\u{1e80b}\"<x`'\u{a08}#A&\u{1f244}\u{102b5}".to_string(), "\u{a5}\u{dc}".to_string(), "7&I\u{16ff1}1$V\u{b7}\u{1d409}%w|X".to_string(), "\u{fffd}\u{105b5}".to_string(), "\u{1aba}\\\u{1d4a5}\u{fffd}s\u{103c8}".to_string(), "\u{fdcf}\u{468}*\u{1ee9a}j\u{1b22}J\u{1932}\u{d5e}=\u{b57}\u{2cad}6\"\u{c01}\u{101a0}?`\u{2b84}I($".to_string(), "$?$J0\u{a5}$\u{11720}\u{23a}\u{be}+]$M:e{".to_string(), "\u{2128}\"g\u{2a37a}\u{c56}\u{1ee57}*$/p\u{468}\u{cb0}j\u{2dcb}{\u{fffd}\u{a5}\u{1f574}d\u{c14}4%/o=v\u{b3f}`\\sIn".to_string(), "\u{aa44}\u{11281}\u{11361}&\u{fffd}\"\u{10eac}h.UE\u{11139}.\u{dca}K\u{a5}d\u{a5}x\u{753}%8#\u{e8}\u{a7}:$\u{1f86f}\u{a5}\u{9aa}".to_string(), "\u{2d92}{\u{1f574}\u{2db8}\u{23a}0\"\u{1f225}\u{2a2c}\u{101a0}\u{a5}\u{fffd}\u{23a}\u{85e}/`<{".to_string(), "\u{1f574}\u{10f6}'\u{c48}./^\u{1e7ee}?\"\u{10cd}\u{1d53c}=o=/3:".to_string(), "\u{1f59}7)\u{1f6f3}\u{1ee75}\u{daf}.\u{ea}0\u{c4}\u{468}\u{1d929}=\u{11ca9}\u{23a}=\u{1f574}*\u{1c24}\u{1ee39}<\u{aaf5}\u{1321}\u{a3}\u{1fae7}\u{fb0}i".to_string(), "P<|\u{a5}\u{a5}j{".to_string(), "~\u{1873}\u{a5}\u{a4b}a\u{a811}r".to_string(), "v/\"\u{23a}\u{1f83}\u{11c76}'".to_string(), "*\u{1193b}\u{aa50}*\u{105a5}?\u{1f574}/\u{1df27}\u{abf2}".to_string(), "D*\"o&?<\u{a5}<\u{a5}\u{df4}.\u{f9a0}C\u{2f94c}}\u{f71}T\u{fffd}\u{118ae}I\u{10a38}\u{2d6f}jV".to_string(), "\u{b712}\u{10763}\u{a1}Z\u{1bc9c}zN\u{a3}q\\\u{ab8}".to_string(), "X\u{10f7c}W\u{23a}\u{bf}\u{ec6}F\u{23a}*E\"/<R{\u{1f574}\u{ff10}\u{1d6d2}y*\u{ce}\u{176e}\u{11460}'\u{1ee4f}".to_string(), "".to_string(), "\u{11d40}\u{c9}<\u{19e2}".to_string(), "_\u{1ee6a}*<V&D\u{1ff7}>\u{1f1a}\u{a5}O\u{23a}\u{11695}\u{1ee57}:\u{1d546}".to_string(), "'O{i\u{1893}\u{c63}\u{23a}\u{d82}\u{c5d}ZB\u{2d27}\u{2dd2}$}F?\u{1f247}\u{11d94}\u{d8}\u{2d70}\u{ab20}\\d".to_string(), "\u{10524}\u{16e43}".to_string(), "yc\u{1e2ef}'\u{10035}:Sdf\u{1c1d}\u{aa2c}\u{11357}\u{fffd}\u{1f113}h\\\u{1f574}//\u{11a2a}=$uNJ\u{1abd}:`{\u{114c4}**".to_string(), "%\u{fffd}\u{114d8}\u{10d16}]\u{10800}^\u{10cd}\u{23a}]".to_string(), "r\u{fe71}<,`{V\u{fb3b}\u{a39}&G:\u{b4b}*\u{10a05}\u{5d9c}%=4".to_string(), "".to_string(), "n{?\u{1132a}'\u{107b2}\u{1d02b}\u{7af}&\u{468}WabZ$\u{111a9}K&\u{1028d}#q=v".to_string(), "*\u{a5}?\u{1eea7}\u{179d}\u{1ee64}Au=\u{1e7e9}$\u{bc8}\"\u{110d7}".to_string(), "\"E$\u{f57}\u{dd6}&\u{fe64})\u{107b3}\u{a5}C\u{11cb1}G<:5\u{11453}\u{23a}?`\u{a7d3}\u{2007}V5\u{d5}\u{16b56}".to_string(), "=".to_string(), "\u{c43}i>\u{1e028}<\u{1bc9c}&\u{f2}$\u{468}L\u{f4}=\u{1315}Iit\u{a3c}\u{fffd}\u{5d6}Y\u{1902}\"\u{fb3e}&\u{fffd}L\u{85e}\u{1132b}".to_string(), "\u{12c0}\u{2441}\u{1e132}$%\u{1128b}\u{11586}m\u{a4}".to_string(), "".to_string(), "dp\u{c94}K\u{aae5}. {=\u{1115e}?i\u{1111b}R%/g\u{2545}\u{107b7}\u{1f186}d\u{1e90a}".to_string(), "\u{10c7}Cx\u{1f574}\u{bae}?.{N\u{23a}%:\u{fffd}/b\u{1940}\u{1d4a6}.\u{ec6}\u{c4}=\u{1093f}\u{23a}r\u{1e7f2}Z".to_string(), "\u{1d872}\u{bd7}<\u{abf7}`\u{85e}<\"\u{20a8}\u{9b8}=\u{1f59}\u{468}\u{aee}.={%".to_string(), "&\u{960}^{uT|/<\u{100d7}\u{10efe}jD\u{a83}\u{1f1a}.=w\u{13b8}$\u{e81}uh&\u{c4b}2\u{1a81}".to_string(), "%\u{109fa}1=g&W\u{10eb0}.,\u{fffd}Y<^\u{10854}}\u{11308}\u{a2f}'[$\u{103c3}\u{10808}a\u{11f08}\u{fb44}'\u{c0}\u{cbc}W/L".to_string(), "\u{16fe4}<\u{10f71}\u{85e}w\u{103a0}?{\u{468}\u{11c03}".to_string(), "{".to_string(), "=}kU(\u{f93b}>{{${\u{1ee39}O&\u{cde}arQ\u{b36}\u{ec6}\u{10c7}\u{1f574}O\u{cdd}Z\u{181fa}\u{810}\\^".to_string(), "\u{a833}/J* %.'\u{afa}".to_string(), "*\"\u{103bc}".to_string(), "?".to_string(), "X=`{O\u{23a}\u{1c23}\u{1113d}\u{11d09}n$&\u{111f1}\u{110c0}\u{cc8}\u{b9c}\u{ea7}\u{c02}=\u{10199}\u{1f574}]\u{a5}".to_string(), "j\u{fe27}2\u{1b6b}f\u{17e2}'?%\u{fffd}{&\u{bcb}U\u{dc3}'Lh".to_string(), "\u{10a3a}\u{fda3}/+$/{\u{df3}s::\u{1816}\"\u{a5}/&5-".to_string(), "\u{303dc}0\u{1248f} \u{e0c}N`\u{99d}<\"' {y'\u{1ee47}\u{dc}6*'&\u{2d27}?\"*".to_string(), "\u{1111a}m\u{d9}\u{1005b}-\u{10b9a}\u{16ad4}`".to_string(), "\u{1b7c}`\u{fffd}&uO&\u{11915}\u{bcd}\u{cf3}\u{114d9}`<'7?{".to_string(), "\u{a03}V$_*<w\u{1ee4f}\u{2d70}*\u{c0}\u{a5}?\u{1f574}Q\u{10808}e<*]\\\u{110ac}$\u{1cf46}/{\u{1f574}\u{11c02}\u{f6}\u{211f6}n\u{468}".to_string(), "YK\u{b9c}\u{faa}\u{1ee5d}<B*%6\u{fffc}8&\u{16f8f}/\u{b02}\u{10b9b}<".to_string(), "\u{10992}\u{1f265}d-\u{105a7}<".to_string(), ".\u{23a}'".to_string(), "\u{6fa4}P\u{fffd}'&{:\u{a5}\u{fd23}\u{174e}\u{1eeae}\u{1f574}\"\u{2088}\u{1e7e2}".to_string(), "\u{23a}u=\\\u{23a}vZ.?\u{11c77}0\u{cd}A.\\\u{468}\u{c8e}\u{112a1}\u{1f574}".to_string(), ".\u{aa4d}\u{d78}d\u{a5}\"\u{23a}*b\u{a0}6)%s\u{fb3e}\u{1057a}\u{a35}O$]\u{b3}\u{1b132}%?*\u{11a5d}W\u{a5}\u{11303}%\u{10b68}=".to_string(), "_\u{fb3b}$\u{1002a}?\u{d5}\u{a5}w\u{19b7}'\u{cef}*\u{fffd}{\u{1f5d}\u{11d61}U\u{10009}._\u{1f574}\u{16ff0}e\u{cdd}".to_string(), "<\u{fffd}\u{23a}\u{11c8a}\u{1087e}c\u{11da0}\u{1f574}\u{104a4}\\\u{1d2c6}Y!?M:$\u{12c2}\u{1da9f}*$\u{1745}\u{fffd}o\u{102eb}\u{1958}.".to_string(), ")\u{1df29}*\u{238}J<\u{b3}\u{1f0f0}<'%$R/&&\u{a59}".to_string(), "\u{23a}\u{1fe1}\u{1244c}$d\\/\u{11435}[JP%/\u{11d53}e".to_string(), "\u{fb2}\u{110dd}\u{16fe0}\u{fa75}\u{11d8b}\u{1e2c9}k\u{ab0e}?$$%u\u{ab5}\u{11d94}/:%\u{10b07}'\u{aa21}\u{fcd2}\u{807}\u{10f10}Z$\u{a5b}".to_string(), "\u{b5}&\u{ffc5}.*{\u{1051b}\u{468}\u{16b18}\u{a5}p$Q9\u{d95}$:\u{23a}\u{aa1e}\u{16e43}\u{ab7d}\u{2b8e}f\u{fffd}'GW\u{fffd}P&".to_string(), "\u{10fb5}?\"QI7\u{dec}\u{1ee6f}\u{b83}d*./*c:\u{fe}\u{17669}Ff\u{a966}\u{1bc94}\u{fffd} \u{109c4}\u{77e}\u{1e8cc}?{<\u{c7}\u{11733}".to_string(), "\u{9ac}'=%\"`*\u{23a}?\u{121ad}\u{11726}\u{c19}Y\u{1ee01}:`\u{a5}\u{fb05}\u{1ee5b}\u{ab6}\u{acd}F&<".to_string(), "%\u{a8}\u{1884}?\u{11204}iWUD\u{1f574}&\u{124a}\u{58f}Z\u{a5}6Z'\u{e0116}".to_string(), "9'\u{11332}\u{dc0}\u{a5}\u{468}S{\"\u{aadd}=]S.\u{1f0c7}\u{1258}#\u{1e14e}\u{1f574}\u{1fae1}*\u{1df2a}\"^GR\u{468}*".to_string(), "\u{ffda}\u{a5}:\u{105bc}D".to_string(), "?\u{f5}G\u{b44}\u{468}\u{10c84}0^1\u{ca0}`\u{110f7}&".to_string(), "\u{fffd}<\u{df2}\u{a5}&$%\"\u{10a17}:*':\u{1269}V\u{11d67}$@\u{fffd}\u{11c41}.^d\u{10b8c}\u{1940}9%|\u{23a}\u{abc2}\u{ab16}".to_string()], vec!["\u{1daa7}".to_string(), ".\u{fffd}$\u{1005c}\u{1e7e8}U\u{ede}".to_string(), "\u{2dd1}<2.\u{a0f}\u{1d4bd}-%`\u{ffcc}\u{317a1}j\"1\u{a5}R\u{12470}\"\u{1915}\u{23a}W\u{c78}H{>%\u{fffd}\u{468}\u{10d0b}\u{a5}\"".to_string(), "p\\:\u{118a1}=<\u{190f}\\JI\u{fb7}\u{1ee59}X\u{385}\u{10e9b}*=$%6\u{11f01}<\u{dd}|\u{abe}\u{23a}\u{1fa77}nv=$".to_string(), "\u{2ff3}\u{eb5}\u{23a}$BE|%\u{a5}\u{a8cf}2".to_string(), "*M,%\\{V.\u{fffd}&\u{1ed06}\u{1d74}q\u{1f8a4}\u{11727}\u{1083c}\u{1f8b0}\u{ffdc}\u{fffd}\u{11c1a}n\u{468}H</%\u{12ca}`\u{11c04}:V".to_string(), "]j".to_string(), "k~\u{1b132}\u{10081}`\\\u{cb1}\"".to_string(), "`\\\u{114d1}\u{1ee39}\u{1f574}F\u{1f802}e=\u{2d27}%\u{1131a}\u{10b5e}E*\" \u{1120b}=\"F\u{394}'F\u{a857}\"\u{10b99}".to_string(), "+\u{a895}\u{468}'*$<\u{1f228}\u{1f70b}\u{1061a}\u{1f808}\u{cb2}:P>{".to_string(), "\u{1f5b}\\\u{1f574}I[x.`?\u{1d11c}\u{468}\u{fffd}\u{10d25}\u{b87}\u{1b43}:\u{102a6}{\u{fffd}'\u{1884}\\\u{f6}.M\u{1246c}~P\u{e2}g".to_string(), "\u{209b}\u{a7d3}_e`\u{c7f}=H.\u{2dd5}CA\"\u{fffd}\u{16ae0}".to_string(), "<\u{cf3}\u{1f8b1}\u{1ee8e}i\\\u{11c04}(\u{d7}=%".to_string(), "}\u{10100}\u{23a}g\u{f97d}xB\\\u{7cd}Y\u{877}\u{da}\u{1ee64}\u{10f73}\u{11c6b}-]c/\u{1e7ea}\u{104a2}?\u{23a}]\u{1ee77}\u{12449}x*<".to_string(), "\"".to_string(), "X\u{1ee4b}\u{ce}\u{b4d}\u{10b99}.R::\u{9d7}\u{1cf36}}M".to_string(), "6e*\u{60c}=*G&*\u{d0c}v\u{1f574}.\u{1ee52}\u{11d68}2N}\u{1f878}T\u{11d67}\u{1b166}:b\u{1e00d}\"Dq\\z ".to_string(), "\u{102e7}\u{1074a}:\u{1ee47}\u{468}".to_string(), "?`.&\u{23a}".to_string(), "\u{116ad}\u{1f0c8}%\u{1d60e}'I{`B\u{23a}6\u{17be}+\u{ba}%s.\u{1083c}".to_string(), "\u{11937}".to_string(), "\u{a739}.>\u{468}\u{a5}&rO/\"x\u{1f574}j?\u{1fda}:<\u{e1}=/\u{1fa8}<E\u{e9d}".to_string(), ".)\u{a5}?\u{1a80}\u{10588}\u{c5}\u{468}\u{116b7}&c%\\\u{a8f}\u{1a33}P&K%/\u{5e1})?rHI\u{10b47}\u{2dae}`a".to_string(), "F\u{10a15}\u{108e5}\u{468}\u{1f7f0}.6.\u{138f}\u{fffd}j{\u{1da9b}\u{1093f}\\l\u{1d52c}\u{1281}\u{c90}k".to_string(), "2=\"v\u{a5}X\"C ~\u{468}'\u{468}\u{468}'\u{2e2a}\u{11363}\u{468}g".to_string(), "$L*`%*{&\u{2b761}/\u{1f853}".to_string(), "\u{11310}\u{16b50}\u{468}t<G\u{793}".to_string(), "{&$=\u{1ee7e}\u{b9c}\u{1d36a}[".to_string(), "\u{de}'\u{23a}\u{11580}'\u{103c9}\\\u{10f01}\u{fffd}'\u{10761}${\u{e1}`\u{ce}F<'*\u{5f4}\u{ed}\u{18e5}\u{10808}\u{468}//]".to_string(), "@X\u{fa77}".to_string(), "C \u{1ca0}\u{12027}%W\u{a5}\u{16f00}\u{11368}<\u{aa19}<".to_string(), "\u{1ee76}*\u{23a}&i:8\u{ab57}m)Y\u{16f1}<\u{a5}\u{d7b8}\u{b9a}$$".to_string(), "v(]\"8".to_string(), "\u{1aff6}\u{23a}\u{b48}\u{23a}$<?`=~".to_string(), "\"\u{1260}\\\u{1a98}q\u{6f4}.B\u{e8e}.L\u{d35}\u{aaaa}^5.\u{1bc83}`\u{e1d}\u{fd}W\u{1fbad}".to_string(), "]\u{a91}*\u{b6d}.=`\"\"A\u{109a6}\u{fffd}\u{fc8d}J\u{aac2}'7\u{b2}\u{1258}\u{11f27}'{Z\u{1f0e4}\u{d0e}\u{a5}".to_string(), ")\\/{\u{101d5}\u{1d50a}".to_string(), "c#4E\u{d82}+\u{1f574}>&\u{aa2a}]2S\\\u{ed}\u{124b}%/U\u{468}\u{1d4aa}/%'\u{b37}T$$\u{1d7a1}".to_string(), "\"o\u{11fb0}%\u{d0}#\u{b2b}\u{1770}\u{c58}\u{fffd}\u{1df28}*\u{315b8}\u{103d1}\u{468}/;\u{468}'!'.l\u{1e854}\u{2dc3}\u{10f3d}\u{ac}\u{c8e}".to_string(), "%\u{d10}p\u{11a50}\u{1ee4d}\u{fffd}?\u{11d40}\"r`y\u{23a}<\u{23a}\u{1fd3}".to_string(), "F\u{1ee5b}H\u{10040}\u{2b042}?St\u{23a}\u{c46}?\"\u{11cae}F'".to_string(), "CJX*\u{1ce0}\u{1a6e}\u{baa}\u{ec8}+$%\"\u{e0}\u{10a06}\u{a5}\u{1038c}\u{1f722}\u{a5}E\u{2dd5}K\u{820}\\F\u{1f574}?".to_string(), "l\\i'\u{10b99}\u{fffd}\u{1f574}\u{11612}Z".to_string(), "\u{c4a}\u{fffd}\u{aab}\u{10838}`\u{1f6f5}\u{1ee51}\u{a32}\u{1bc87}\u{10baf}\u{19d3}&E\u{fb39}w`\"\u{fffd}\u{c56}\u{1f574}\u{23a}\u{f3}Y\u{1ee36}".to_string(), "\u{125b}>%\u{11333}.\"\u{c85}\u{2ff4}'\u{468}\u{11350}\u{1e7e1}&\u{fb43}\u{10fe2}\u{1b165}\u{1ee42}\u{10a38}\u{16b87}\u{1173b}".to_string(), "\u{dbd}\u{17d3}\u{1f0a3}\u{23a}0*Ke!Kz\u{11d3a}\u{1003d}{\u{fa}.&\u{1ee4b}\u{1e7ee}\u{ebd}\u{10fc8}=/+\u{468}\u{fffd}\u{fffd}".to_string(), "\u{10023}\u{1f574}<\\\u{c6}g\u{2eb6c}\u{11d61}\u{11332}\u{a9f}Xs\u{1375}\u{dd4}\\\u{d4f}%.Q{.&\u{fffd}\u{1b151}5Z".to_string(), "*`@\u{1f6a}\u{ac}\u{468}\u{a5}\u{fb7e};= y.".to_string(), "J&\u{1f56}{\u{d7}\u{708}/h\u{11938}N\u{39c}\u{a5}.\u{e2}\u{a5}{\u{1fbf4}'s\u{a5}_\u{c48}\u{11350}fT?\u{10817}Y".to_string(), "\u{23b3}\\/c\u{468}\u{c7}~^/.w%?:C\u{1136b}/`".to_string(), "v<\u{2d70}\u{a5}=\u{94b}'\u{1312}\u{1d6b9}\u{1ee35}\u{13fb}+\u{fffd}<\u{1d49f}g\u{10817}\u{19d3}/\u{23a}*./%\u{1f574}\u{1f574}\u{ac4}*".to_string(), "=\u{ca1}\u{468}\u{baa}=fd=\u{dbd}\u{fffd}\u{a08}\u{12b2}\u{12473}$\u{19a0}x>\u{ca}}".to_string(), "\\=\u{e9}{\u{9f2}-`".to_string(), "\u{bc}\u{2008}\u{1ed10}\u{1f6fa}\u{11321}/\u{1f574}\u{5de}'\u{a9d7}k8\u{11461}\u{e94}\u{1ee49}\u{f8}\u{10b5a}\u{1191c}z\u{10b35}=y\u{1ee8b}".to_string(), ")%\u{cd}&\u{30a6}`{\u{16f0}&\"S\u{107b9}\u{468}<\u{112f7}\u{fb40}_9=t={\u{a5}\u{1f24}\u{fe35}:\"\u{1f574}".to_string(), "\u{1f7ac}<\u{1344}<\u{1ee49}\u{b03}\u{12b4}\u{f2}{\u{11d46}\u{b32}:\u{a5}/o\u{16b55}{".to_string(), "".to_string(), "i\u{ca}{=\u{16f92}{\"$%\u{1f574}%C\u{afe}\u{58f}\u{468}]{%?\u{543}Zc\u{1d2d3}\u{11710}\"%=\u{71b}".to_string(), "\u{14487}//*D?c".to_string(), "q\u{e1}wB\u{b1}\u{1f0d5}6&&\\\u{1d33f}Ly\u{c7}:\u{1f6f0}\\\\.\u{1ee97}m".to_string(), "n:={>{\u{1800}\u{1ee49}\u{468}w\u{1d59e}\u{1eea9}d\u{e29}S\u{2dce}|.\"?\u{1002a}\u{1bc83}//%\"\u{12fbd}".to_string(), "\u{1aff3}($\u{101d5}".to_string(), "o\u{10a53}4*:=\u{a5e}.".to_string(), ":\u{17f6}=\u{23a}OX\u{1ec7a}\u{11ac9}\u{b39}<\u{1bc09}\u{10ce6}\u{ec3}WB\u{10acd}\u{23a}X*$*b\u{fffd}//\u{fffd}=\u{11d91}*".to_string(), ":".to_string(), "\u{1d033}Pw=b\u{10990}AXvs\u{ea5}t;\u{10519}::.\u{a2}\u{1e124}<*!\u{10d36}".to_string(), "{\u{119a0}\u{afe}\u{1ac3}\u{1b2be}\u{fffd}\u{468}<U:h&NN'?<<{h\u{468}5K/".to_string(), "\u{23a}=\u{d95}\u{2d075}\u{e84}6j\\\u{1f0ef}\u{cd}\u{1d508}\u{23a}\u{1ee3b}\u{a5}\u{1f229}Z\u{16874}%$%".to_string(), "/\u{322a}\u{ab6}".to_string(), "\u{468}.\u{e89}\u{a4d}d`\u{a5}/".to_string(), ":.'\u{2172}'=\u{1ee47}?n".to_string(), "\u{fffd}j\u{1f574}\u{16b2a}\u{1ee21}\u{a3c}\u{1d4cb}<d.\u{1811}i'\u{1ca9}V\u{1e020}\u{83c}[\u{10915}\u{1e8cb}\u{104b4}'.\u{19d9}\"\u{aae8}\u{f3}?#\u{fb44}Y".to_string(), "\u{5f2}$\u{11ab9}\u{f4a}t\u{23a}\u{23a}\\/'{\u{1e950}".to_string(), ":.v\u{18b21}:e,i&\u{1093f}p{".to_string(), "\u{23a}\u{11909}\u{fe69}?\u{1cc2}\u{a5}{.\u{1093f}\u{2d33}\u{16af2}m\u{1132d}\u{1d540}".to_string(), "\u{1e2a1}7\u{102cd}%/?\u{a5}x\u{a5}~\u{11da4}\u{10c7}*;\u{11332}\u{1e2e2}w<<".to_string(), "'\\\u{23a}\u{1c0a}\u{2d85}4\u{468}$*\u{b9a}$\u{11089}\u{1b165}\u{1128b}\u{ab16}s{X?7".to_string(), "{".to_string(), "\u{a03}e\u{23a}T\u{deb}{i\u{10c8c}$v\u{d1}\u{1a92}\u{fffd}\u{468}>\u{1d42a}8*H\u{115b9}*q`\u{1fa0}".to_string(), "<&?B\"\u{1f574}kV<\u{a02}SC/|\u{1eea5}Q\u{10d31}.}:\u{8f3}\u{109e0}\"\u{1f6e9}\u{cc}t\u{2e88}".to_string(), "6\u{11837}q\u{1aff1}qT\u{bf}\u{1f260}P\u{1cb0}\u{1e92f}'=?`=\u{1ee59}$%".to_string(), "@\u{ae1}\"1\u{75c}\u{afe}\u{1d4a2}\u{b0f}\u{2db1}\u{10854}&\u{a5}\u{ab0}\u{ffcd}\u{10b9}Y\u{207e}M<\u{105b0}".to_string(), "\u{ec}\u{a5}/:\u{1130f}\u{109cb}{/\u{110d3}j\u{176c}M\u{1f875}<?\u{a2}\u{1f574}\u{11fce}\u{1f574}\u{22ab}Pn/\u{a47}8\u{ba9}=".to_string(), "eT<\u{1affd}\u{1744}".to_string(), "${om]\u{616}\u{1170a}<".to_string(), "k\u{fb16}{".to_string(), "\"6".to_string(), "Z\u{1f574}\u{a681}/\u{1fa38}\u{1135d} ".to_string(), "\u{2002}W\u{1ee5d}'z\u{1bd9}\u{10aec}?j7/\u{a5}'\u{1d52c}\u{b93}5\u{11327}.\u{1112d}:&$\u{1128b},\u{ae8}\u{fda3}?\u{110bf}M".to_string(), "1\u{105ae}\"\u{fcc}\"\u{e0}.\u{1f57}q~\u{1fa9f}`rV\u{109bc}g\u{1f18}h\u{10765}xH\u{dd9}i\u{b0}>6".to_string(), "\u{a3}ezc\u{11302}\u{19d3}\u{10a39}g:\u{2b0c1}`\u{1135f}\u{a25}\u{119a3}`S*:\u{1ee57}O\u{1f7c}".to_string(), "\u{10a42}\u{85e}\u{13448}".to_string()], vec!["w/T=Dy\u{ea5}\u{a5}\u{1ee57}'\u{10527}\u{a5}=&#e\u{1ee7e}&Fd".to_string(), ":c\u{16b8a}\u{cbf}V=\u{edf}Z<&\u{1f574}\u{11d18}".to_string(), "QEYSV\u{d70}%\u{1df25}M)\u{1bc43}|nD\u{12c5}&./_".to_string(), "<\u{1f574}.\u{1ee92}X^\u{12473}\u{3bbe}.\\\u{11294}\u{1f574}".to_string(), ".2\\\u{1ee61}\u{1ee59}&\u{10746}bI\u{1000f}\u{12b4}\u{a5}\u{cb}\u{2796}\u{1e2ff}".to_string(), "".to_string(), "\u{8d4}*\u{119a3}\u{b95}\u{468}\u{1f0c2}\u{468}r\\:\u{111e7}'\u{c3d}`".to_string(), "*$\u{1fbf8}0\u{9b2}\u{468}*`\u{2d8b}t\u{a86b}\u{1d88b}z$\u{189fb}{\u{a01}\u{1244e}=\u{1f0ee}?&\u{1e7e5}\u{12470}\"\u{11199}<\u{168bf}'\u{11b02}%\u{fb38}".to_string(), "6\u{12b2}*\\\u{a5}Rj\u{1a94}I\u{ced}\u{468}%\u{fffd}'T\u{fc5}\u{ec6}Y&'\u{1e7f5}\u{1d512}7\u{1002a}\u{1f9d}\u{468}\u{216f}#*\u{a7d3}".to_string(), "?.\u{11cb5}\u{fcf}\u{10a13}\u{2db0}\u{1e57}/\u{1f574}F$\u{de}c\u{1721}\u{a5}\u{23a}?`\u{bec}\u{ee}\u{10b4b}\u{11d3a}\u{e84}\u{fb44}\u{b82}Z^\u{a10}".to_string(), "/$Y{/={\\\u{10e8b}>6\u{1cf88}P\u{1b150}\\\u{11aa1}\u{b06}y\u{23a}\u{1fb9}\u{468}:]\u{2e81b}4\u{1d87f}\u{112c7}\u{fffd}\u{1ee5f}*T".to_string()], vec!["%-%s'T{Eg\u{1a74}\u{1a58}`=H\u{ca}".to_string(), "`3:<\u{1d521}a\"%\u{ab16}\u{651}\u{1132b}\u{2de0}{8+\u{1322}\u{10a17}/\u{11fb0}\u{11d91}\"\u{1aff0}\u{2dce}=L".to_string(), "=`x\u{ea}\u{1f574}_$\u{11310}\u{1eea3}\u{fb03}\u{1e831}*8\u{11d08}\u{11350}O\u{9b0}".to_string(), "$)\u{1e7f6}\u{2dba}@\u{10762}`".to_string(), "\"?\u{1ee51}\u{fb40}\u{aa}U?\u{11915}\u{10feb}=:\u{fffd};".to_string(), "\u{c9d}\u{fdcf}\u{11f0c}/\u{1258}\u{85e}%\u{fbf}\u{e92}=\u{23a}7\u{1e2ea}\u{1f574}\u{11370}._$\u{a5}\u{1d365}\u{ae};\u{1385}\u{cbd}".to_string(), ";\u{a5}\u{fd9d}\u{ffe2}.\u{fffd}=\u{ea9}_\u{468}".to_string(), "V\u{1f0cd}(?\u{1f5b}\u{fd8d}c\u{11f40}t\u{f3}`\\\u{2b6d}cT\u{1d47d}\u{f3}:\u{19d9}\"&*".to_string(), "\u{fffd}\u{f2}{".to_string(), "*2\u{16ff0}{\u{ac8}\u{1d4b8}\u{acb}\u{ea}\u{1aff0}<MQ=5><D/b9".to_string(), "\u{fe5a}\u{10085}\u{11d42}\u{1128c}\u{f0}\u{fffd}E\u{f5}\\\u{a32}\u{11da5}\u{11d67}?\u{105bc}{".to_string(), "\u{b42}\u{23a}'\u{16f55}&x-\u{23a}\u{fe63}sK\u{a59}\u{ba}j\u{11c1b}&U\u{1e957}XE\u{a5})\u{1ee39}\u{2ed4}|".to_string(), "\u{105bc}\u{192a}\u{a94e}$%{w\u{468};\u{fffd}#%.".to_string(), "\u{10594}".to_string(), "{s\u{1f574}\u{468}".to_string(), "\u{fffd}q:H\u{1f104}X\u{1712}\u{ae8}&\u{16fe0}\u{468}\u{1ff9}\u{468}\u{1f574}\u{37d}5q".to_string(), "\u{1f859}\\$\u{ffd3}\u{2d70}\u{23a}\u{b1}\u{16ac9}/`.\u{16b7}\u{11c56}?\u{b37}:\"\"\u{1eeb6}:P".to_string(), "K'D\u{a840}\u{fffd}4\u{afa}{\u{2daa}&?T\u{1d546}\u{1101f}".to_string(), "*6%SU%\u{10b7d}c@$\u{11f38}\u{468}?.\u{a36}\u{1e115}-%\u{11c97}\u{1da9e}\u{102cf}\u{11d65}l\u{12c0}<n*".to_string(), "<\u{11fd0}\u{95e}\u{fe}/W:B\u{a5}\u{aa0c}P\u{104a7}=$\u{13fd}{\u{dc1}\u{fffd}-&\u{fb02}?".to_string(), "\u{32e}m'R&\\\u{ae7}\u{aa86}\u{23a}\u{11b03}{=".to_string(), "".to_string(), "L%Ce'\u{2d0b}'\u{110e2}`N&\u{1fad1}'".to_string(), "\u{23a}\u{fffd}\u{ac7}?%\u{468}\u{1ee13}\u{23a}\u{1810}`?\u{a03}\\\u{1180a}\u{b1a} ,\u{1eef1}".to_string(), "'*&\u{b82}$\u{1f105}G E.P\u{1ee76}\u{10595}$\u{10af2}:\u{a5}\u{23a}E\u{a8a1}<\u{f3}\\\u{abf6}/'".to_string(), ":\\".to_string(), "O*{x.\u{d9c}$".to_string(), "\u{13ab}*<K\u{ba4}/&8v&$\u{a30}[\u{5e4}\">\u{1f574}/\u{10cd6}\u{1e2dd}\u{abc}\u{986}\\".to_string(), "LC'\u{1895b}\u{1ee5d}<\u{a39}\u{fffd}\u{11c3e}\u{b1}\u{83d}0\u{1fc9}P\u{115a5}'/P\\*F'\u{110f9}\u{1fbf2}\u{12c0}%%/=?".to_string(), "\u{11f29}\u{d2}\u{634}\\\u{fffd}\u{11acb}:\u{aa53}N\u{ea5}\u{d48}\u{1b132}".to_string(), ":\u{1fda}.>\u{1e01d}:\u{fc7c}`\u{ea5}.1T?f.\u{a1}A\u{db}0*&\u{d4}/T".to_string(), "S.mpA/\\S&\u{11d97}".to_string(), "\u{11d41}\u{1faf8}o40\u{101e3}".to_string(), "D\\B\u{103d4}W\u{16ff0} %\u{1f75}\u{1376}{D\u{1f53f}:=\u{cae}?\\|".to_string(), "'".to_string(), "\u{1fbf}\u{fffd}{/k\u{1e005} $$#\u{2ff3}<\u{10005}{9\u{fffd}\u{2b88}".to_string(), "L&\\m/p\u{19d5}\u{85e}\u{105bb}\u{a5}#pV\u{10a43}\u{1bc3d}\u{ec6}'\u{11596}\u{12509}&\u{a4bf}T\u{2f80c}\u{11a1c}#\u{1faf0}\u{1e013}#'\u{b5f}<".to_string(), "{{t \u{11d93}\u{ec8}E\u{e9d}u{".to_string(), "\u{10b8c}\u{16f3}tPD\u{b4b}\u{11a17}\u{98f}%\u{1f574}".to_string(), "\u{9f8}og".to_string(), "'\u{1fc2}\u{128b}r$\u{10047}[\u{1c98}\u{110da}\u{d10}<\u{11938}U\u{a2e}\u{d5}?\u{108f4}6".to_string(), "\u{81b}\u{1e2d1}\\.\u{468}\u{b4}=`\u{12b2}\u{31c5}\"\"".to_string(), "%`Ax?\u{11c95}\u{ab22}&\u{1940}:".to_string(), "\u{118ff}\u{d02}\u{11f42}\u{ea2}\u{a5},tz~".to_string(), "\u{bb}\u{23a}?\u{2c8c}<\u{1a02}/\u{a5}\u{11d97}\u{1190d}\u{125b}\u{fffd}\u{23a}\u{b1}\\X\u{9dd}`\u{1ee03}\u{d2}\u{10857}\u{12471}\u{f6}W\u{1f574}R\u{be}\u{2071}".to_string(), "\u{fffd}E\u{23a}\u{612}<:O</".to_string(), ":E%".to_string(), "\u{fffd}\"Y\u{fffd}#\u{1d1cf}\u{10b6f}\u{a5e}\u{a7d0}:\u{ab2e}p".to_string(), "di\\.\u{13fd}[*\\\u{bb3}/=`:*S\u{11a39}<*".to_string(), "\u{cde}{\"\u{128a}e$\u{1f5cd}<\u{213a}<%'\u{1f785}\u{13413}\u{31c1a}.&\u{d93}\u{12b2}N\u{1134c}\u{1fa15}<\u{c68}".to_string(), "Z\u{2f836}\u{1130a}\u{9b8}\u{1ff4}P\u{966}\u{c9}?\u{a2c2}<\u{1078c}n\u{1ee77}\u{1fd8}\u{23a})n\u{a3}\u{23a}\u{11301}".to_string(), "<\u{1f574}\u{1f7e7}\u{1e139},\u{1fbf3}\u{a5}\u{2db9}\u{b10}P\u{16b5d}e\u{10838}\u{1083c}{\u{a5}\u{a5}\u{1133c}\u{1ee52}$\u{11c32}\u{1b132}U\u{384}\u{1f574}\u{168a0}=/.\u{11438}DN".to_string(), "\u{1ee14}".to_string(), "e\u{10180}\"".to_string(), "".to_string(), "\u{12472}&\u{1ee61}\u{468}\u{23a}\u{11d56}\u{a7d3}\u{11c05}{\u{1f5b}\u{543}".to_string(), "\u{fe60}:6`\u{b01}?L`6M\u{16846}\u{2b7d}\u{12c5}".to_string(), "2\u{a5}\u{fffd}'\u{daa}\\O`\u{17af}%\u{1d14a}j".to_string(), ":%.7\u{a5}{\u{11689}".to_string(), "\u{1011b}/\u{d8}**\u{1f18}&\u{b4d}<\\_\u{115a0}\u{a1}$:\u{18d5}%.~{\u{10829},?`8+\u{10745}\u{468}\u{10a16}@\u{a1}\u{2d27}".to_string(), "n%\u{2bf3}\u{2071}\u{fffd}\u{10cd}.h%uJ\u{1f574}\u{9e2}\u{fffd}\u{1b150}.\u{a33}@\u{a2}x\u{dde}\u{b2d}I".to_string(), "'\u{fffd}\u{10841}%\u{11498}^\u{e81}.\u{104dd}'".to_string(), "?L\u{1ee03}n\u{468}`J'\u{1d64}\u{1e00d}\u{bca}\u{1f250}\u{d1}A".to_string(), "<'\u{194e}\u{d7c0}\u{a5}<fU\u{468}?=TR[%eD".to_string(), "\u{cc1}%\u{a3c}\u{468}Z\u{11a5b}N<\u{119e3}\u{fffd}\u{468}-\u{104bc}M\u{109c6}\u{11f46}\u{a5}\u{12473}\u{1f574}\u{1f574}\u{e4}".to_string(), "&\\\u{a5}\"<\u{11d58}\u{bb3}\"<\u{b7}\u{a831}\u{23a}\u{468}\u{e01ef}\"!?\u{1770}Z".to_string(), "G\u{1f59}U".to_string(), "\u{1c3f}:3\u{1f836}\u{118d8}\u{119a5}\u{10537}\u{fe}2\u{1b167}\"\u{1f574}X*$\"\u{1ee4f}\u{23a}\\\u{111e4}4?.7<`\u{10b5a}<h\u{512a}\u{b4}-".to_string(), "\u{10f26}*\u{1abb}/\u{23a}\u{fffd}=\u{1b47}\u{fe5d}\u{468}?'\u{109c1}X|".to_string(), "`C,\u{1d528}4@\u{1f6e6}\u{ed2}!\"\u{ac3}.`;U&\u{fffd}*\u{1f7e7}".to_string(), "G='\\\u{23a}\u{2ede}3$\u{d4d}\u{a5}:US\u{1ec8c}\",$*oF.&\u{1fd6}\u{3077}\u{11c9f}\u{fffd}w".to_string(), "?\u{fffd}a\u{10aeb}\u{f2}\u{f4}F\u{1f31}\u{fb3e}U[\u{b57}4K\u{1931}.\u{2d2d}\u{110c1}".to_string(), ",&\u{19d1}&\\\u{1faf3}*5+\\\u{10800}S\u{fc38}".to_string(), "&1\u{1344b}\u{fffd}(\u{bf3}\\\u{10a45}\u{1b2fb}\u{1b7e}`\u{10a55}\u{468}\u{468}\u{a5}\u{baa}`\u{9ce}d\u{1f71d}$\u{1f574}\u{ae}\u{119a3}{B\u{119b0}:}!".to_string(), "=o\u{1e124}\u{5d8}a2+/|\u{1063b}\u{179d}%".to_string(), "L\u{1e7eb}`nl{se\u{2de84}\u{3122}k\u{e7}.k{\u{fb15}\u{e93}q\u{a96b}\u{12080}g\u{1e039}\u{1d362}\"".to_string(), "\u{1b032}%{6M\u{a5}\\?&*\u{30481}R\u{1e7ee}\u{ab4e}&?\u{a5}\u{11d67}%\u{101a0}'I\u{b05}%".to_string(), "<\u{fffd}\u{10597}\u{a5}{\u{1b132}\u{10a0c}\u{1f000}\u{1cf58}k\u{e7}\u{1d544}\u{f7a}/\u{a5}\u{98c}=/\u{1ee7e}\u{1090d}:\u{1b6a}\u{1f59}$\\&\u{23a}^\u{cc6}".to_string(), "\u{2dba}\u{1fac4}\"R.\\".to_string(), "".to_string(), "\u{a47}'S\u{468}\u{c59}X\u{105bc};K\u{1d50a}`/\u{16a60}\u{a5}\u{a7d3}*$cP\u{9c8}$'\u{1f0bd}t".to_string(), "\u{1d53d}\u{ec6}B\u{1136b}T\u{562}\u{23a}\u{d81}\u{10100}&Q{".to_string(), "``B\\\u{10191}\u{5a2}\"\u{10581}\u{fffd}".to_string(), "\u{23a}c&q`O\u{10847}m\u{a5}\"".to_string(), "\\/".to_string(), "\u{1f574}\u{1055a}\u{a7d9}+/\u{112c3}\u{1133b}\u{fe4f}\u{d8a}\"V\u{10fe9}\u{1133f}F\u{11333}\u{1d540}\u{580}\u{fffd}a\u{1038f}Yv\u{468}\u{1f574}\u{fb05}\u{2dbb}".to_string(), "dAW.\u{742}\\d(:L`\u{2447}!E_\u{1f9be}\u{1f016}]'\u{fa61}\u{bbf}\u{8fe} \u{fc4}^\u{aa07}\u{1e89d}\u{b9c}_".to_string(), "\u{d9}\"\u{23a}\u{1255}H\u{fffd}\u{e4}/$\u{a9ef}\u{fe54}/.\u{18d02}\u{1f850}\u{a8e3}\u{fffd}0r@:\u{2ddd}.{\u{b55}\\4\\\u{a5}D".to_string(), "\u{279fa}s\u{b9}\u{1e103}%\u{468}\u{12ba}\u{1059b}=.%:`\u{b9}\u{1f0ce}=0\u{d7d0}_`\u{a493}\u{468}i{\u{2ebb}".to_string(), "\u{b73}z[,".to_string(), "H\u{1134d}/v$@*\u{9fb}{\u{1ee09}\u{11d01}e8\u{17d6}&:.*N\u{2fae}\u{ebd}\u{1190c}".to_string(), "`".to_string(), "\u{1f081}f\u{1f796}\u{c1}{\u{ed7}\u{1d45f}\u{12c5}^:\u{11938}\u{17e0}\"\u{2185}\u{fffd}%t?/k{%6`%".to_string(), "\u{fffd}\"\\cX?\u{53b}/K\u{711}\u{1745}\u{dd2}\u{ad7f}6\"&&\u{5ef}\u{c59}\\B\u{1d546} \u{1c44}*v*%{'\u{cb7}]".to_string()], vec!["$\"\u{10f80}".to_string(), "\u{1f261}\u{23a}\u{1f574}$:^\u{1f9d}<R\u{aa19}S=\u{107b6}".to_string(), "\\\u{1ee39}\u{f52}A\u{11338}`.\"a\u{468}\u{31dd}%G\u{1054c}Zo\u{fffd}=k*\\\\`\u{11339}\u{23a}\u{ab26}=\u{959}3SO".to_string(), "\u{11638}\u{11c42}\u{2dd2}u\u{862}?D*\u{bbf}y\u{468}\u{1f728}\"K{\u{1ee7e}%C\u{11f39}".to_string(), "\u{1aa2}I\u{ffc2}\u{16e8c}<<,5={G\u{d7}\"\u{1e2ff}&W`\u{bd0}\u{9c1}\u{1735}\u{1b056}\u{1f70c}v:&+\u{468}S%\u{860}".to_string(), "\u{7f8}\u{cc6}~\"\u{1368}\u{a08}:\u{1f574}<[\u{c5d}\u{a4b}:9C/\u{468}Z?\u{10f1b}\u{128c}\u{31521}k'yl\u{10ba9}\u{468}r".to_string(), "\u{23a}\u{f9b3}\\$\"9\u{a5}q\u{1d063}\u{1f12}\u{10000}\u{be}".to_string(), "$=\u{145a9}\u{c36}%\u{11909}{\u{1cf5e}".to_string(), ">z<:\u{fb}?/?j\u{87f}\u{2441}\u{11348}x<f\u{101a0}\u{c6d}*".to_string(), "\u{acc}<\u{a5}".to_string(), "\u{16fe1}\u{10cd}{\u{b8}\u{11347}$\u{1d4a5}\u{1f574}$\u{1f574}\u{1f574}g\u{1eeb9}/{\u{990}LY\u{10b5}\u{ab09}\u{11286}\u{2c993}\u{23a}\u{16a4f}\u{2d41}\u{d5}".to_string(), "\u{1f6fb}L\u{1f574}t\u{a5}ZBd\u{1f574}^\u{1f7f0}%:\u{11ee9}0\u{1f59}=\u{b39}:\u{16ae6}x\u{1b151}*\u{1bc87}.?o".to_string(), "S\u{fce}.(\u{1925}=$9\u{18dd}\u{1f574}*\u{fcdc}\u{468}`\u{10808}E\u{b4c}+\u{fffd}~5\u{11288}\u{3236e}=\u{e012e}\u{1ee6f}\u{fe72}\u{10684}Z\u{fffd}\u{11d67}".to_string(), "\u{1b2ac}\u{fffd}~\u{1d9e5}/\u{11f44}\u{aa47}*\u{abdf}<\\:?q=#\u{610}1\"\u{ed8}\u{1fbc}5\u{a02}'(L^q\u{da}".to_string(), "\u{3208}%t\u{2082}".to_string(), "\u{107b7}%\u{121ed}\u{d8e}N\u{b2}h\u{aa}\u{468}='W\u{11241}\u{1f82}\u{1ee39}:%y}@\u{a4c}$\u{dde}\u{a5}\u{a5}\u{1190f}".to_string(), "\u{16b5c}\u{1f574}\u{a5}\u{a5}t@\u{a0f}\u{1ee57}\u{105bc}\\\u{aa6}'`${/\u{1f6f2}\u{a5}\u{468}\u{cf3}/\u{468}\u{1e016}{\u{10c18}j\u{11d90}\u{1ee39}Qu\u{abd}".to_string(), "{'{J\u{866}5.\u{f7d}f|.g\u{23a}~{$=k=Q/\u{10cc5}+\u{fb44}-\u{2ecf}?".to_string(), "'\u{468}{=\u{be}\u{184c}".to_string(), "\\".to_string(), "*:\u{1df25}:\u{1ee42}\u{10594}\u{c0a}\u{17e9}\u{aaea}\"\u{c4d}</\u{c1a}&Z".to_string(), "\u{1f0a1}.`\u{aa}v&\u{11ac1}Y\u{10a57}<gVz\u{11015}\u{120dc}?\u{fd}\u{1ee34}\"M(\u{1e001}\u{1e043}#'{\u{ef}\"".to_string(), "$\u{1f805}e\"GVSEy\u{1f574}<\u{a5}".to_string(), ".$\u{1ee85}\u{18ba}\u{10b55}\u{fb56}8\u{1ee71}VU\u{1d4a6}\u{1d4a6}\u{a5}%\u{1f574}=nH\"{\u{a5}:\u{1885}".to_string(), "\u{3151}:\u{fffd}Jg$\u{a5}".to_string(), "'<\u{1f6e0}\u{1d4d4}'\"/l\u{c90}p0\u{1cf32}2\u{c8f}\u{10808}:\u{a578}\u{b8f}\u{f9}\u{df4}\u{1b151}{\u{fcf}".to_string(), "\u{10e9e}Jjy\u{d55}\u{c90}\u{1e298}%v{\\\u{468}9\u{1002e}\u{1e822}".to_string(), "".to_string(), "\u{1d7b6}d\u{1ee7e}\u{1d470}\u{abbf}\u{23a}\u{208e}".to_string(), "\u{a5}(+Q\u{11d3c}`\u{a7}".to_string(), "<\u{10121}{==\u{1f238}\u{cb}:U*&\u{16f4}:\u{a833}8`&=".to_string(), "{\u{1b74}\u{1ee92}:\u{bd0}\u{10573}".to_string(), "C\"^)\u{31fea}\u{10fe2}:d\u{2040}\u{3127}{2b&\u{179a0}C={r.\u{ba8}`e\u{23a}DO\u{1f574}".to_string(), "\u{d91}\u{a5}".to_string(), "".to_string(), "\u{1d693}\u{fffd}\u{a5}'&u\u{b90}\u{1e08f}\u{2fd2}\u{fffd}1W\u{1f574}P\u{d5}\u{10742}4".to_string(), "\u{f71}\u{1f574}\u{a4c}&\u{1cf41}\u{ce}\u{fffd}\u{ce2}?m\u{12dc}G'E\"6%l\u{1f574}*\u{dca}\u{a5}".to_string(), "\u{2d1c}".to_string(), "G2{\u{468}%'\u{1fac2}\u{10f7a}$\u{c8}\u{23a}l\u{ab12}7".to_string(), "V3\u{e6}4b\u{38a}\u{c4b}\u{1a6b}\u{fe25}/\u{11caf}".to_string(), "\u{10cea}n'\u{1f574}&\u{10131}>\u{303a}@\u{a5}\\\u{11aee}\u{e4}]\u{a8a4}\\\u{468}v.$Q\u{1ee6f}<\u{1128f}&\u{cd}".to_string(), "\u{1ec7f}\u{cd5}\u{1d717}{\u{10523}".to_string(), "\\\u{11d6a}{\u{1f574}\u{1e023}\u{a5}{\u{a5}\u{7cf}\u{2d27}\u{a5}\u{1f574}'\u{10740}<\"`f\u{1d50d}".to_string(), "&\u{a5}<\u{23a}\\&<%\u{f8}\u{d7b4}\u{ab}\u{11d62}k\u{d5a}\u{98f}\u{1bc97}\u{10b99}".to_string(), "+`\u{a32}\u{f0}r@&s$6?\u{11d3c}X\\p|i&".to_string(), "\u{2daa}{\\/\\=x\u{fffd}j\\\u{1c36}\u{a5}\u{1f9e1}\u{1ec8c}\u{1d546}1\u{1f574}`Dpr^i:\u{468}\u{116a5}\u{11288}".to_string(), "\u{10b9b}e\u{acd}\"".to_string(), "\u{ce}\u{1e017}\u{c7}\u{23a}\u{fffd}\u{a89b}O\u{180a}*\u{a492}\u{1ee5d}?${\u{1ee70}\u{bb}\u{9d7}<\u{a06}q\u{18a9a}C".to_string(), "\u{1f59}<\u{a5}W/J\u{1f5b}\u{23a}T\"\u{1c3b}Sc\u{10ea2}\u{a5}\u{a951}\u{10059}\u{1f574}{\u{16b6d}\u{c3}m".to_string(), "\u{11b03}*\u{23a}\u{11728}=\u{10d39}\u{b5}\u{b6}pG8s&y\u{c46}\u{2b21}4\u{1f574}/1\u{1e024}\"".to_string(), "\u{10d31}\u{cd5}.".to_string(), "\\]hR\u{1f574}.\u{23a}\u{b1}\\W[~C\u{fb41}$\u{a5}\u{1ee64}U\\%<'\"\u{1f574}r/\"\u{680}\u{b1}Q".to_string(), "".to_string(), "=Ssn=&w'i\u{10a1c}\u{ec0}\u{2d7f}X\u{1f574}\u{1f053}\u{23a}{".to_string(), "7\u{2df3}V=/\u{1da9b}A\u{ea}/.{^/j".to_string(), "ae%\u{dee}}\u{10a2a}/\u{fffd}4\u{23a};'{F$\u{10799}P\u{a02}\u{10f19}v\u{1054d},\u{10b6d}".to_string(), ":6".to_string(), "C\u{f2}\u{aed}&H`\u{fb2e}\"*!m&\u{aa57}\u{fffd}".to_string(), "./C\u{12c3}\u{16adf}M?\u{fb41}\u{ff95}\u{660}\u{d7b9}\u{1eea8}\u{23a}`\u{104a7}\u{10c09}uV<\u{9b6}{\u{fe}*6<*\u{10594}\u{fffd}<?`".to_string(), "5\u{23a}\u{1f19}\u{dbb}<:J\u{23a}*s\u{b40}k\u{b48}Y<D\u{fb2}".to_string(), ".\"\u{ba9}j`\u{16a4e}('\u{1258}%/\u{10390}\u{1ee96}i\u{bb2}\u{c3e};".to_string()], vec!["\u{b88}:%g;\\\u{c4b}D\u{f82}{&\u{468}l\\\u{b82}g\u{be}&\u{c98}\u{1f260}\u{10373}a\u{1ee7a}=&\u{23a}**p".to_string(), "\u{1003d}Ks\u{1ee29}^\u{468}4%\u{a7}".to_string(), "\u{1f574}\u{fb40}<=h`\u{1f574}.%N#\"".to_string(), "=(\u{a5}\u{a08}o@".to_string(), "F:N".to_string(), "\u{1cfb5}\u{11288}".to_string()], vec!["b\u{16ca}^\u{1149c}<Z\u{1948}\u{11c52}\u{11909}\u{1ee42}\\e\u{1f1a}=<`\u{2f8a0}\u{1d525}&WLB?h\u{10368}\u{9cd}\u{12c5}$.".to_string(), "<\u{2ff1}&\u{125b}iU/".to_string(), "*7\u{1f574}{\u{1ee89}\u{fea9}B_\u{2183}\u{a2f}\u{dc4}\u{d78}'m=\u{205f}".to_string(), "I\u{10342}}\u{cbc}a&i\u{1aff0}T\u{2d59}?\u{a5}?`.>\"`".to_string(), "\u{1093f}\u{1eef0}\u{10eb0}\u{37a}/G\u{176b}#\u{2f82}\u{11f4e}%\u{2d70}<\u{bb0}\u{b87}".to_string(), "gB\u{1083c}\u{d5}".to_string(), "\u{1b155}\u{1708}\u{1b1e}p\u{12b4}\u{1eea8}\\".to_string(), "".to_string(), "\u{1e7eb}Q\u{bc7}d".to_string(), "\"{*`LMMk=\u{1affe}*`j".to_string(), "v'{\u{f2}\u{a39}.".to_string(), "Ro/LQ\u{10a9f}\u{23a}<'`\u{f9}Y\u{13147}]w?hY".to_string(), "\u{1f21a}&\u{16b57}\u{a5}\u{10b7d}\u{23a}\u{1ee49}\u{1f5b}\".\u{11a3c}?\u{a94c}=z*\u{1284}\u{1f574}\u{1fc3}".to_string(), "\u{10a05}\\\u{a5}\u{11360}\u{10cfd}\u{1ee22}\u{1e4d9}\u{11339}\u{311b}\u{468}\u{10f15}\u{1fc6}\u{c3f}$\u{ccb}<\\.c".to_string(), "\u{10c23}\u{1f574}\\\u{468}\u{11d3a}<*\u{1f574}<y\u{e0}'\u{2c0c2}:&\u{1d237}\u{1933}qr&\u{30e7}\\t':J".to_string(), "=\u{fffd}9a{\u{1158f}y :\u{1eea1}'K\u{fffd}=\u{fffd}\u{1f80b}:\u{1ee70}\u{136c}\u{11319}\u{1ff4}".to_string(), "".to_string(), ",\u{11310}%`!\u{a86}".to_string(), "\u{fe22}\u{a5}3\u{ca2}\u{10d34}z/\"\u{119c5}_\u{10d35}\u{11cab}\u{c0}\u{a5}\u{23a}\u{ea2}7V\u{2d70}|".to_string(), "\u{23a}?\u{bf4}\u{468}h<".to_string(), "\\\u{1f574}*\u{16b87}*\u{a74}\u{11812}.\u{1f0c6}`\u{108ad}\u{1057c}\u{a5}\u{105b6}%%_{?\u{2441}{\u{8eb}\u{fffd}Y'\u{468}{\u{12472}\u{607}\u{fe71}b".to_string(), "\u{ffac}\u{1c44}\u{105bb}\u{1e912}\u{1d30f}".to_string(), "\u{105ac}=".to_string(), "X\u{23a}\u{127c}\u{a3c}$*?\u{f2}$\u{10b8c}2`\u{c2b}\u{e0}.\u{1287}<\u{1d53b}\u{23a}".to_string(), "\u{125c}\u{879}I\u{1f574}\u{23a}B\u{1f574}%*\\$%\u{b3c}\u{fffd}\u{a7}\u{16ad2}%.\u{10e72}\u{fffd}%\u{df3}\u{1804}v<%\u{bc7}\u{10003}\u{fed4}".to_string(), "\u{b5c}'W\u{10fe6}A:uT\u{9aa}\u{11f13}\u{1f0d1}\"\u{116c4}?\u{1831}\u{1d05e}\u{1227b}\u{1fb53}{**?\u{a48}\u{ac7}d".to_string(), "{\u{fffd}\u{16b59}".to_string(), "?=i\u{1e812}\u{108e0}'\u{bd0}$O\u{10f1b}\"*\\\u{aab}nq\u{11c43}".to_string(), "\u{e9}\u{176f}r\u{c0e}aD{`\u{1f857}".to_string(), "\u{e3}O`L\u{b7}".to_string(), "/\u{1fada}\u{ee}\u{b2}F&\u{a4c0}M\u{107c}=\\i\\\u{fb21}Y\u{114c4}\u{fffd}?=\u{e22}\u{bb}&^`\u{11c81}\u{1f574}-\u{11210}".to_string(), "*%q$\u{c9}\u{a5}\u{10b68}\u{dd6}-\u{468}\u{1cf46}&\u{1d79c}\u{cf2}\u{11d90}\u{468}/U\u{37c}5\\".to_string(), "*\"R\u{2b25}W\u{1d2e7}{\u{1e2f5}{J=".to_string(), "=>{\u{1f574}%/\u{10a2c}$o:\u{a0}v\u{10f0a}`".to_string(), "\u{1f574}{!\\\\)O\u{1d53e}\u{10461}<`\\\u{b47}\u{1f863}H\u{de}.".to_string(), "\u{17980}%\u{b6e}\u{468}\u{1d41d}\u{d7c4}\u{1681c}o%`_\u{1eef1}\u{214a}\u{fdad}\u{11322}\u{a642} K`".to_string(), "\u{d0}[\u{1873}h<\u{1e95f} \u{b5}\u{1ed3d}".to_string(), "T\u{1f574}\u{110f3}\u{bef}\u{11288}".to_string(), ">F/R.\\\u{fffd}\u{11373}\u{109bd}\u{a0}[\u{b3e}L\u{ac8}{\u{11925}+n\\=\u{11cb3}{&\"B`\u{103a}.R\u{d7da}\u{df3}".to_string(), "\u{b32}\u{144f4}\u{1ee3b}\u{a5}\\\\\"<\u{dbd}\u{1635}.\u{c81}\u{1e13a}\u{1f6f5}`:\u{a7}\u{11708}\u{2071}\u{11582}\u{a42}\u{30411}\u{1f11}>\u{10576}h\u{1134c}&:)h".to_string(), "\u{dcf}y.\u{b4}\u{21f5}Q\u{11206}%\u{23a}\u{2050}\u{868}%\u{a9}\u{11cb0}\"\u{a72d}\u{12453}\u{114d4}Yb\u{1f574}E\u{1e143}g\u{1f574}".to_string(), "\u{17f7}".to_string(), "\u{10752}\u{a0f}*\u{1ee4f}".to_string(), "?\u{11d3a}\u{2bce}\u{aad}6\u{10d36}Ba={\u{f6}\u{1f574}<8&/\u{a0a}:.\u{1d4a2}\u{468}_5\u{fffd}".to_string(), "\u{1f574}_x\u{13441}\u{fffd}{\u{861}\u{c59}\u{1fbf7}?{\u{1f73f}\u{ffe1}*\u{1e14f}\u{bcb}\u{fffd}z/'/\"".to_string(), "\u{16b64}\u{ae7}?".to_string(), "\u{11c92}?".to_string()]];
    let inserted_join: Vec<String> = vec![];
    let installed_join: Vec<String> = vec!["\u{11f20}:".to_string(), "5$%".to_string(), "\u{144fd}`x\"ve\u{2ff8}\u{a7d3}Yt\u{ffc6}:<\\\u{1108e}\\$\u{cc}6".to_string(), "\u{1d4b8}k=%\u{a7}E\u{1a30}o<8\u{f9fd}:_\u{1072a}".to_string(), "..=\u{c2d}\u{a86}''O%".to_string(), "\u{1f574}g:".to_string(), "\u{a06}i\u{1ee62}\u{396}vLcV\u{e6}\u{c90}".to_string(), "{\u{81a}^\u{11705}*}\u{1faf3}G&\u{10a06}/\u{e58}\u{11310}\u{10d18}\u{10383}\u{23a}".to_string(), "\u{112f1}\"p\u{a5}Q/vp\\\u{1f783}\"\u{d82}{%\u{1e8d1}\u{1054b}8&#\u{d47}'\u{10b7d}O\\Z".to_string(), "\u{468}\u{9b8}\u{1f8b0}\u{ab0a}*\\?\u{b26}\u{2e94}\u{1ee42}\u{12c3}\u{12474}\u{10011}<8`".to_string(), "doa\u{391}\u{ac2}\\.2\\=\u{11661}\u{17e7}<\u{ab16}5=*\u{1ee51}{j.\u{1faf4}*\u{1df28}('{".to_string(), "\u{19e4}X\u{11350}Z\u{23a}\u{dec}\u{a81}\u{10152}".to_string(), "[=^$\u{468}\u{11a13}\u{1f016}\u{aaec}%\u{a08}\u{1f574}.q".to_string(), "\u{1e2a6}\u{a5e}\u{10102}?l\"\"\u{1258}\u{1ee4b}\u{1e82a}4gc\u{d1a}O=$5\u{16f35}*\u{11332}\u{a51}\u{1f574}=\u{1ee4f}F\u{10905}<".to_string(), "\u{fdcf}3W".to_string(), "\u{10852}\u{cb8}.m\u{c63}\\\"` ".to_string(), "\u{108ae}k{=$Mi{:|%\u{fffd}Q*]?!\u{23a}&nwW]\u{105b3}Y\u{23a}<q&<*".to_string(), "&<{{\u{10592}+P".to_string(), "\u{1971}*\u{1d4a2}\u{1df04}\u{b10}j\":\u{1940}?\u{1cc1}\u{1e875}2\u{10734}%\u{ffc7}f\u{1f574}\u{7ef}".to_string(), "'W?8\u{119dc}\u{110bf}p\u{1f574}ny\u{fffd}'\u{a5}k\u{1fcb}\u{d7bb}?Jr\u{a5}? |F<`\u{b0f}=:\u{1ea7}".to_string(), "?*\"<`U?F\u{fffd}\u{16a49}\u{13b2}\u{1e7e1}%=\u{a491}\u{b62}\u{a7d1}\u{1f574}\u{ba3}n{\u{fffd}t".to_string()];
    (channel, cached_joins, inserted_join, installed_join)
}

/// Decode every `\\u{..}` / `\\n` / `\\\\` / `\\"` escape to the character it
/// denotes.
///
/// # Why the comparison below cannot be raw text
///
/// Rust's `Debug` for `str` decides per character whether to print it literally
/// or as an escape, and that POLICY has changed across Rust releases. The
/// corpus was recorded by an older toolchain: at exactly one position out of
/// 13,696 — the scalar `U+1134D`, a Grantha combining sign — the archived text
/// and today's `Debug` disagree about whether to escape, while denoting the
/// identical character.
///
/// Comparing raw text would therefore fail on a toolchain upgrade while nothing
/// about the counterexample had changed, which is a flake, not a check.
/// Decoding both sides first removes the rendering policy from the comparison
/// and leaves the CHARACTERS — every real difference still shows, and this is
/// what caught the earlier reconstruction defect.
fn decode_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                // `\u{XXXX}`
                let mut hex = String::new();
                if chars.peek() == Some(&'{') {
                    chars.next();
                    while let Some(&h) = chars.peek() {
                        chars.next();
                        if h == '}' {
                            break;
                        }
                        hex.push(h);
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(decoded) => out.push(decoded),
                    None => {
                        out.push_str("\\u{");
                        out.push_str(&hex);
                        out.push('}');
                    }
                }
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// ★ ANTI-VACUITY, against the corpus FILE rather than a copy of it.
///
/// Rebuilds the `# shrinks to` payload from the reconstructed values and
/// requires it to equal the line the corpus actually contains, both sides read
/// through [`decode_escapes`] so the comparison is over CHARACTERS rather than
/// over one toolchain's escaping policy. A promoted test comparing against a
/// literal pasted beside it would agree with itself; this one cannot — and it
/// already caught a real reconstruction defect, at one character out of 13,696.
#[test]
fn the_recorded_hot_store_case_is_what_the_corpus_recorded() {
    let corpus = include_str!("../proptest-regressions/hot_store_spec.txt");
    let (recorded_seed, recorded_text) = corpus
        .lines()
        .find_map(|l| l.strip_prefix("cc ")?.split_once(" # shrinks to "))
        .expect("the corpus still carries its entry");
    assert_eq!(
        recorded_seed, "9209dda1ef18f70480507bce69ed4f2957bbad8352216ad7f2f361fa36a94ede",
        "the corpus entry this test was promoted from is no longer the one in the file"
    );

    let (channel, cached_joins, inserted_join, installed_join) = recorded_case();
    let rebuilt = format!(
        "channel = {channel:?}, mut cached_joins = {cached_joins:?}, inserted_join = \
         {inserted_join:?}, installed_join = {installed_join:?}"
    );
    assert_eq!(
        decode_escapes(&rebuilt),
        decode_escapes(recorded_text),
        "the reconstructed bindings do not denote the recorded counterexample"
    );
    // The lengths of the RAW renderings are reported when they differ, so a pure
    // escaping-policy divergence stays visible rather than being silently absorbed.
    if rebuilt != recorded_text {
        eprintln!(
            "note: the reconstruction denotes the same characters but renders differently ({} \
             chars vs {}) — an escaping-policy difference between the recording toolchain and \
             this one, not a difference in the counterexample",
            rebuilt.chars().count(),
            recorded_text.chars().count()
        );
    }
}

/// ★ THE DISPOSITION: this recorded case is EXCLUDED by its own property's
/// precondition.
///
/// The entry's four bindings (`channel`, `mut cached_joins`, `inserted_join`,
/// `installed_join`) match exactly one property in this file,
/// `install_join_should_cache_installed_joins_separately` — and the recorded
/// values do NOT satisfy its `prop_assume!`. Measured, not inferred:
/// `inserted_join` is the EMPTY join `[]`, and `cached_joins`'s first group is
/// also empty, so `!cached_joins.contains(&inserted_join)` is false and the
/// case is rejected before a single assertion runs.
///
/// # Why the precondition is right, and what it protects
///
/// `put_join` DE-DUPLICATES, explicitly and deliberately — both branches insert
/// only `if !occupied.get().iter().any(|j| j.as_slice() == join)`
/// (`rspace++/src/rspace/hot_store.rs`). The property's expectation prepends
/// the inserted join UNCONDITIONALLY, which models the non-duplicate case
/// alone. On the recorded input the store holds 9 joins where that expectation
/// predicts 10 — measured directly while diagnosing this. The `prop_assume!`
/// scopes the property to the case its expectation actually models; it is not
/// masking a defect.
///
/// A first reading suggested a coverage gap — "the de-dup behaviour this
/// counterexample found is excluded, therefore untested". That was CHECKED and
/// is FALSE: `put_join_should_not_allow_inserting_duplicate_joins` covers it
/// directly. The hypothesis is recorded next to its refutation, because "we
/// looked and there is no gap" and "nobody looked" are indistinguishable from
/// outside.
#[test]
fn the_recorded_case_is_excluded_by_its_propertys_precondition() {
    let (_, cached_joins, inserted_join, installed_join) = recorded_case();
    assert_ne!(
        inserted_join, installed_join,
        "the first conjunct of the `prop_assume!` DOES hold for the recorded case"
    );
    assert!(
        cached_joins.contains(&inserted_join),
        "★ the recorded case is supposed to VIOLATE `!cached_joins.contains(&inserted_join)`. If \
         it no longer does, the entry has stopped being inert and must be promoted as a live case \
         of `install_join_should_cache_installed_joins_separately` instead."
    );
    assert!(
        inserted_join.is_empty(),
        "the recorded `inserted_join` is the EMPTY join, which is what collides with the empty \
         group already in `cached_joins`"
    );
}

/// `put_join` is IDEMPOTENT on the recorded (duplicate) input, and
/// `install_join` is undisturbed by it.
///
/// The seed can never reach an assertion through its own property, so this is
/// what earns it its place: the concrete outcome on this specific adversarial
/// input — 452 join strings across 9 groups, astral-plane scalars, replacement
/// characters and embedded escapes — a shape no future generator run will
/// realistically reproduce. The general law is
/// `put_join_should_not_allow_inserting_duplicate_joins`; this is that law on
/// the one input the corpus actually recorded.
#[test]
fn the_recorded_case_does_not_duplicate_the_join_it_reinserts() {
    let (channel, cached_joins, inserted_join, installed_join) = recorded_case();
    let before = cached_joins.len();

    let (_, hot_store) = fixture();
    hot_store.set_state(HotStoreState {
        continuations: HashMap::new(),
        installed_continuations: HashMap::new(),
        data: HashMap::new(),
        joins: HashMap::from_iter(vec![(channel.clone(), cached_joins.clone())]),
        installed_joins: HashMap::new(),
    });

    hot_store.put_join(&channel.clone(), &inserted_join.clone());
    hot_store.install_join(&channel.clone(), &installed_join.clone());

    let cache = hot_store.snapshot();
    let joins = cache.joins.get(&channel).expect("joins").clone();
    assert_eq!(
        joins.len(),
        before,
        "re-inserting a join the channel already holds changed the join count"
    );
    assert_eq!(joins, cached_joins, "the join list was reordered or rewritten");
    assert_eq!(
        cache
            .installed_joins
            .get(&channel)
            .expect("installed joins")
            .clone(),
        vec![installed_join],
        "the installed join is kept separately and exactly once"
    );
}
