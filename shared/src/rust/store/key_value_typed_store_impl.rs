// See shared/src/main/scala/coop/rchain/store/KeyValueTypedStoreCodec.scala

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use super::key_value_store::KvStoreError;
use super::key_value_typed_store::KeyValueTypedStore;
use crate::rust::store::key_value_store::KeyValueStore;
use crate::rust::BitVector;

#[derive(Clone)]
pub struct KeyValueTypedStoreImpl<K, V> {
    store: Arc<dyn KeyValueStore>,
    phantom_data: PhantomData<(K, V)>,
}

impl<K, V> KeyValueTypedStoreImpl<K, V>
where
    K: serde::Serialize
        + for<'a> serde::Deserialize<'a>
        + Clone
        + Eq
        + std::hash::Hash
        + std::fmt::Debug,
    V: serde::Serialize + for<'a> serde::Deserialize<'a> + Clone,
{
    pub fn new(store: Arc<dyn KeyValueStore>) -> Self {
        Self {
            store,
            phantom_data: PhantomData,
        }
    }

    /// Exposes the underlying untyped store so callers can batch writes
    /// across multiple typed stores that share one LMDB environment — see
    /// `crate::rust::store::lmdb_key_value_store::batched_put`.
    pub fn raw_store(&self) -> &Arc<dyn KeyValueStore> { &self.store }

    pub fn encode_key(&self, key: &K) -> Result<BitVector, KvStoreError> {
        Ok(bincode::serialize(key)?)
    }

    pub fn decode_key(&self, encoded_key: &BitVector) -> Result<K, KvStoreError> {
        Ok(bincode::deserialize(encoded_key)?)
    }

    pub fn encode_value(&self, value: &V) -> Result<BitVector, KvStoreError> {
        Ok(bincode::serialize(value)?)
    }

    pub fn decode_value(&self, encoded_value: &BitVector) -> Result<V, KvStoreError> {
        Ok(bincode::deserialize(encoded_value)?)
    }

    // See shared/src/main/scala/coop/rchain/store/KeyValueTypedStoreSyntax.scala
    pub fn get_one(&self, key: &K) -> Result<Option<V>, KvStoreError> {
        let values = self.get(&vec![key.clone()])?;
        match values.split_first() {
            Some((first_value, _)) => Ok(first_value.clone()),
            None => Ok(None),
        }
    }

    pub fn get_batch(&self, keys: &Vec<K>) -> Result<Vec<V>, KvStoreError> {
        self.get(keys)?
            .into_iter()
            .zip(keys)
            .map(|(value_opt, key)| {
                value_opt.ok_or(KvStoreError::KeyNotFound(format!(
                    "Error when reading from KeyValueStore: value for key {:?} not found.",
                    key
                )))
            })
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn get_unsafe(&self, key: &K) -> Result<V, KvStoreError> {
        self.get_one(key)?.ok_or(KvStoreError::KeyNotFound(format!(
            "Error when reading from KeyValueStore: value for key {:?} not found.",
            key
        )))
    }

    pub fn put_one(&self, key: K, value: V) -> Result<(), KvStoreError> {
        self.put(vec![(key, value)])
    }

    pub fn put_one_if_absent(&self, key: K, value: V) -> Result<bool, KvStoreError> {
        self.store
            .put_one_if_absent(self.encode_key(&key)?, self.encode_value(&value)?)
    }

    pub fn put_if_absent(&self, kv_pairs: Vec<(K, V)>) -> Result<(), KvStoreError> {
        let keys: Vec<K> = kv_pairs.iter().map(|(k, _)| k.clone()).collect();
        let if_absent = self.contains(keys)?;
        let kv_if_absent: Vec<_> = kv_pairs.into_iter().zip(if_absent).collect();
        let kv_absent: Vec<_> = kv_if_absent
            .clone()
            .into_iter()
            .filter(|(_, is_present)| !is_present)
            .map(|(kv, _)| kv)
            .collect();

        self.put(kv_absent)
    }

    pub fn contains_key(&self, key: K) -> Result<bool, KvStoreError> {
        let results = self.contains(vec![key])?;
        Ok(*results.first().unwrap_or(&false))
    }

    pub fn get_or_else(&self, key: K, else_value: V) -> Result<V, KvStoreError> {
        match self.get_one(&key)? {
            Some(value) => Ok(value),
            None => Ok(else_value),
        }
    }

    pub fn any_value<F>(&self, mut predicate: F) -> Result<bool, KvStoreError>
    where F: FnMut(&V) -> Result<bool, KvStoreError> {
        let mut matched = false;
        self.store.iterate_while(&mut |_, value_bytes| {
            let value = self.decode_value(&value_bytes)?;
            if predicate(&value)? {
                matched = true;
                Ok(false)
            } else {
                Ok(true)
            }
        })?;
        Ok(matched)
    }
}

impl<K, V> KeyValueTypedStore<K, V> for KeyValueTypedStoreImpl<K, V>
where
    K: serde::Serialize
        + for<'a> serde::Deserialize<'a>
        + Clone
        + Eq
        + std::hash::Hash
        + std::fmt::Debug,
    V: serde::Serialize + for<'a> serde::Deserialize<'a> + Clone,
{
    fn get(&self, keys: &Vec<K>) -> Result<Vec<Option<V>>, KvStoreError> {
        let keys_bit_vector = keys
            .iter()
            .map(|key| self.encode_key(key))
            .collect::<Result<Vec<_>, _>>()?;
        let values_bytes = self.store.get(&keys_bit_vector)?;

        let values = values_bytes
            .iter()
            .map(|value_opt| {
                value_opt
                    .as_ref()
                    .map(|value| self.decode_value(value))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(values)
    }

    fn put(&self, kv_pairs: Vec<(K, V)>) -> Result<(), KvStoreError> {
        let pairs_bit_vector = kv_pairs
            .iter()
            .map(|(key, value)| {
                let encoded_key = self.encode_key(key)?;
                let encoded_value = self.encode_value(value)?;
                Ok((encoded_key, encoded_value))
            })
            .collect::<Result<Vec<(BitVector, BitVector)>, KvStoreError>>()?;

        self.store.put(pairs_bit_vector)?;
        Ok(())
    }

    fn delete(&self, keys: Vec<K>) -> Result<(), KvStoreError> {
        let keys_bit_vector = keys
            .iter()
            .map(|key| self.encode_key(key))
            .collect::<Result<Vec<_>, _>>()?;
        self.store.delete(keys_bit_vector)?;
        Ok(())
    }

    fn contains(&self, keys: Vec<K>) -> Result<Vec<bool>, KvStoreError> {
        let keys_bit_vector = keys
            .iter()
            .map(|key| self.encode_key(key))
            .collect::<Result<Vec<_>, _>>()?;

        let results = self.store.get(&keys_bit_vector)?;
        Ok(results.iter().map(|result| result.is_some()).collect())
    }

    fn collect<F, T>(&self, mut f: F) -> Result<Vec<T>, KvStoreError>
    where F: FnMut((&K, &V)) -> Option<T> {
        let store_map = self.store.to_map()?;
        let mut result = Vec::new();

        for (key_bytes, value_bytes) in store_map {
            let key = self.decode_key(&key_bytes)?;
            let value = self.decode_value(&value_bytes)?;

            if let Some(item) = f((&key, &value)) {
                result.push(item);
            }
        }

        Ok(result)
    }

    fn to_map(&self) -> Result<HashMap<K, V>, KvStoreError> {
        let mut result = HashMap::new();
        let store_map = self.store.to_map()?;

        for (key_bytes, value_bytes) in store_map {
            let key = self.decode_key(&key_bytes)?;
            let value = self.decode_value(&value_bytes)?;
            result.insert(key, value);
        }

        Ok(result)
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.store.non_empty() }
}

#[cfg(test)]
mod tests {
    use heed::EnvOpenOptions;

    use super::*;
    use crate::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

    fn new_store() -> KeyValueTypedStoreImpl<String, i64> {
        let dir = tempfile::tempdir().unwrap();
        let path = Box::leak(Box::new(dir)).path();
        let mut builder = EnvOpenOptions::new();
        builder.map_size(10 * 1024 * 1024);
        builder.max_dbs(1);
        let env = Arc::new(unsafe { builder.open(path).unwrap() });
        let mut wtxn = env.write_txn().unwrap();
        let db = env.create_database(&mut wtxn, None).unwrap();
        wtxn.commit().unwrap();
        KeyValueTypedStoreImpl::new(Arc::new(LmdbKeyValueStore::new(env, db)))
    }

    fn seeded_store() -> KeyValueTypedStoreImpl<String, i64> {
        let store = new_store();
        store
            .put(vec![
                ("one".to_string(), 1),
                ("two".to_string(), 2),
                ("three".to_string(), 3),
            ])
            .unwrap();
        store
    }

    #[test]
    fn key_and_value_codecs_round_trip() {
        let store = new_store();
        let key = "some-key".to_string();
        let encoded_key = store.encode_key(&key).unwrap();
        assert_eq!(store.decode_key(&encoded_key).unwrap(), key);

        let encoded_value = store.encode_value(&-42i64).unwrap();
        assert_eq!(store.decode_value(&encoded_value).unwrap(), -42);
    }

    #[test]
    fn decoding_garbage_bytes_is_a_serialization_error() {
        let store = new_store();
        let result = store.decode_key(&vec![0xff, 0xff]);
        assert!(matches!(result, Err(KvStoreError::SerializationError(_))));
    }

    #[test]
    fn get_preserves_key_order_and_marks_missing_keys() {
        let store = seeded_store();
        let values = store
            .get(&vec![
                "two".to_string(),
                "absent".to_string(),
                "one".to_string(),
            ])
            .unwrap();
        assert_eq!(values, vec![Some(2), None, Some(1)]);
    }

    #[test]
    fn get_one_and_get_or_else_handle_missing_keys() {
        let store = seeded_store();
        assert_eq!(store.get_one(&"one".to_string()).unwrap(), Some(1));
        assert_eq!(store.get_one(&"absent".to_string()).unwrap(), None);
        assert_eq!(store.get_or_else("two".to_string(), 99).unwrap(), 2);
        assert_eq!(store.get_or_else("absent".to_string(), 99).unwrap(), 99);
    }

    #[test]
    fn get_unsafe_errors_on_missing_key() {
        let store = seeded_store();
        assert_eq!(store.get_unsafe(&"three".to_string()).unwrap(), 3);
        assert!(matches!(
            store.get_unsafe(&"absent".to_string()),
            Err(KvStoreError::KeyNotFound(_))
        ));
    }

    #[test]
    fn get_batch_requires_every_key_to_exist() {
        let store = seeded_store();
        assert_eq!(
            store
                .get_batch(&vec!["one".to_string(), "two".to_string()])
                .unwrap(),
            vec![1, 2]
        );
        assert!(matches!(
            store.get_batch(&vec!["one".to_string(), "absent".to_string()]),
            Err(KvStoreError::KeyNotFound(_))
        ));
    }

    #[test]
    fn put_one_overwrites_and_put_one_if_absent_does_not() {
        let store = new_store();
        store.put_one("k".to_string(), 1).unwrap();
        store.put_one("k".to_string(), 2).unwrap();
        assert_eq!(store.get_one(&"k".to_string()).unwrap(), Some(2));

        assert!(store.put_one_if_absent("fresh".to_string(), 10).unwrap());
        assert!(!store.put_one_if_absent("fresh".to_string(), 20).unwrap());
        assert_eq!(store.get_one(&"fresh".to_string()).unwrap(), Some(10));
    }

    #[test]
    fn put_if_absent_only_writes_missing_keys() {
        let store = seeded_store();
        store
            .put_if_absent(vec![("one".to_string(), 100), ("four".to_string(), 4)])
            .unwrap();
        assert_eq!(store.get_one(&"one".to_string()).unwrap(), Some(1));
        assert_eq!(store.get_one(&"four".to_string()).unwrap(), Some(4));
    }

    #[test]
    fn contains_and_contains_key_report_membership() {
        let store = seeded_store();
        assert_eq!(
            store
                .contains(vec!["one".to_string(), "absent".to_string()])
                .unwrap(),
            vec![true, false]
        );
        assert!(store.contains_key("two".to_string()).unwrap());
        assert!(!store.contains_key("absent".to_string()).unwrap());
    }

    #[test]
    fn delete_removes_only_named_keys() {
        let store = seeded_store();
        store
            .delete(vec!["one".to_string(), "absent".to_string()])
            .unwrap();
        assert_eq!(store.get_one(&"one".to_string()).unwrap(), None);
        assert_eq!(store.get_one(&"two".to_string()).unwrap(), Some(2));
    }

    #[test]
    fn collect_projects_and_filters_entries() {
        let store = seeded_store();
        let mut doubled_evens: Vec<i64> = store
            .collect(|(_, value)| {
                if value % 2 == 0 {
                    Some(value * 2)
                } else {
                    None
                }
            })
            .unwrap();
        doubled_evens.sort_unstable();
        assert_eq!(doubled_evens, vec![4]);
    }

    #[test]
    fn to_map_returns_every_typed_entry() {
        let store = seeded_store();
        let map = store.to_map().unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("three"), Some(&3));
    }

    #[test]
    fn any_value_scans_until_a_match() {
        let store = seeded_store();
        assert!(store.any_value(|value| Ok(*value == 2)).unwrap());
        assert!(!store.any_value(|value| Ok(*value > 100)).unwrap());
        assert!(matches!(
            store.any_value(|_| Err(KvStoreError::InvalidArgument("boom".to_string()))),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn non_empty_and_raw_store_reflect_the_backing_store() {
        let store = new_store();
        assert!(!store.non_empty().unwrap());
        store.put_one("k".to_string(), 1).unwrap();
        assert!(store.non_empty().unwrap());
        assert!(store.raw_store().non_empty().unwrap());
    }
}
