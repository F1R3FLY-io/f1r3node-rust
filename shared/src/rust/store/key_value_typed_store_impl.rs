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
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Clone, Default)]
    struct MemoryStore {
        values: Arc<Mutex<BTreeMap<Vec<u8>, Vec<u8>>>>,
    }

    impl KeyValueStore for MemoryStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
            let values = self.values.lock().unwrap();
            Ok(keys.iter().map(|key| values.get(key).cloned()).collect())
        }

        fn put(&self, pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
            self.values.lock().unwrap().extend(pairs);
            Ok(())
        }

        fn put_one_if_absent(&self, key: Vec<u8>, value: Vec<u8>) -> Result<bool, KvStoreError> {
            let mut values = self.values.lock().unwrap();
            if values.contains_key(&key) {
                return Ok(false);
            }
            values.insert(key, value);
            Ok(true)
        }

        fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> {
            let mut values = self.values.lock().unwrap();
            Ok(keys
                .into_iter()
                .filter(|key| values.remove(key).is_some())
                .count())
        }

        fn iterate(&self, f: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> {
            for (key, value) in self.values.lock().unwrap().clone() {
                f(key, value);
            }
            Ok(())
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            for (key, value) in self.values.lock().unwrap().clone() {
                if !f(key, value)? {
                    break;
                }
            }
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> {
            Ok(self.values.lock().unwrap().clone())
        }

        fn print_store(&self) -> Result<(), KvStoreError> { Ok(()) }

        fn non_empty(&self) -> Result<bool, KvStoreError> {
            Ok(!self.values.lock().unwrap().is_empty())
        }

        fn size_bytes(&self) -> usize {
            self.values
                .lock()
                .unwrap()
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum()
        }
    }

    fn typed_store() -> (Arc<MemoryStore>, KeyValueTypedStoreImpl<u32, String>) {
        let raw = Arc::new(MemoryStore::default());
        let typed = KeyValueTypedStoreImpl::new(raw.clone());
        (raw, typed)
    }

    #[test]
    fn encodes_and_decodes_values() {
        let (raw, typed) = typed_store();
        let encoded_key = typed.encode_key(&7).unwrap();
        let encoded_value = typed.encode_value(&"seven".to_string()).unwrap();

        assert_eq!(typed.decode_key(&encoded_key).unwrap(), 7);
        assert_eq!(typed.decode_value(&encoded_value).unwrap(), "seven");
        assert!(typed.decode_key(&vec![1]).is_err());
        assert!(typed.decode_value(&vec![1]).is_err());
        assert!(typed.raw_store().as_any().is::<MemoryStore>());
        assert!(raw.as_any().is::<MemoryStore>());
    }

    #[test]
    fn stores_gets_and_deletes_values() {
        let (_, typed) = typed_store();
        assert!(!typed.non_empty().unwrap());
        assert_eq!(typed.get_one(&1).unwrap(), None);
        assert_eq!(typed.get_or_else(1, "fallback".into()).unwrap(), "fallback");
        assert!(!typed.contains_key(1).unwrap());

        typed.put_one(1, "one".into()).unwrap();
        typed
            .put(vec![(2, "two".into()), (3, "three".into())])
            .unwrap();

        assert!(typed.non_empty().unwrap());
        assert_eq!(typed.get_one(&1).unwrap(), Some("one".into()));
        assert_eq!(typed.get_unsafe(&2).unwrap(), "two");
        assert!(matches!(
            typed.get_unsafe(&9),
            Err(KvStoreError::KeyNotFound(_))
        ));
        assert_eq!(typed.get_batch(&vec![1, 2]).unwrap(), vec!["one", "two"]);
        assert!(matches!(
            typed.get_batch(&vec![1, 9]),
            Err(KvStoreError::KeyNotFound(_))
        ));
        assert_eq!(typed.get(&vec![1, 9]).unwrap(), vec![
            Some("one".into()),
            None
        ]);
        assert_eq!(typed.contains(vec![1, 9]).unwrap(), vec![true, false]);
        assert_eq!(typed.get_or_else(1, "fallback".into()).unwrap(), "one");

        typed.delete(vec![1, 9]).unwrap();
        assert_eq!(typed.get_one(&1).unwrap(), None);
    }

    #[test]
    fn inserts_only_absent_values() {
        let (_, typed) = typed_store();
        assert!(typed.put_one_if_absent(1, "one".into()).unwrap());
        assert!(!typed.put_one_if_absent(1, "changed".into()).unwrap());
        typed
            .put_if_absent(vec![(1, "changed".into()), (2, "two".into())])
            .unwrap();

        assert_eq!(typed.get_unsafe(&1).unwrap(), "one");
        assert_eq!(typed.get_unsafe(&2).unwrap(), "two");
    }

    #[test]
    fn scans_and_collects_values() {
        let (_, typed) = typed_store();
        typed
            .put(vec![
                (1, "one".into()),
                (2, "two".into()),
                (3, "three".into()),
            ])
            .unwrap();

        assert!(typed.any_value(|value| Ok(value == "two")).unwrap());
        assert!(!typed.any_value(|value| Ok(value == "missing")).unwrap());
        assert!(matches!(
            typed.any_value(|_| Err(KvStoreError::InvalidArgument("stop".into()))),
            Err(KvStoreError::InvalidArgument(_))
        ));

        let mut collected = typed
            .collect(|(key, value)| (*key % 2 == 1).then(|| (*key, value.clone())))
            .unwrap();
        collected.sort();
        assert_eq!(collected, vec![(1, "one".into()), (3, "three".into())]);

        let values = typed.to_map().unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values.get(&2), Some(&"two".to_string()));
    }
}
