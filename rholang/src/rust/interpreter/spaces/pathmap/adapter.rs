//! The EPathMap adapter of the PathMap spaces (P5e, plan decision 9.3).
//!
//! The semantics of a PathMap value are logical. A PathMap value is a set of paths, and a path
//! is a list of path elements. An [`EPathMapAdapter`] turns the representation of a PathMap value
//! into its [`LogicalPathMap`] and back, so the re-rooting of a delivery
//! ([`reroot`](super::reroot::reroot)) never reads a representation.
//!
//! [`HeadEPathMapAdapter`] reads the `EPathMap` of this tree. That `EPathMap` holds only `ps`,
//! which keep their source order and can repeat (`collection_normalize_matcher.rs`). The sorter
//! sorts each entry but keeps the list order (`expr_sort_matcher.rs`). So the adapter does this:
//!
//! - `decode`: the path of each entry `e` is `elems(e)`
//!   ([`path_elements`](super::path_codec::path_elements)). The adapter orders the paths by
//!   their entry keys ([`entry_key`]) and keeps the first entry of each key, in the set mode.
//! - `encode`: the entries in key order, each in its canonical entry form
//!   ([`canonical_entry`]). The `locally_free` set is the union of the sets of the entries. The
//!   `connective_used` flag is the disjunction of their flags, and the remainder is `None`.
//!
//! The canonical entry form writes a path of one element `e` bare when `e` is not a plain list.
//! It writes every other path as a plain list. So `elems(canonical_entry(p)) = p` for every
//! path of closed elements, and `decode(encode(m)) = m` for every logical map of closed
//! elements (`PathMapPrefix.v`, `elems_canon` and `head_roundtrip`).
//!
//! The EPM1 representation of `feature/mettail` (`serialized_paths = 8`, `trie_snapshot = 9`)
//! adds the map mode, in which each path holds a value. Its adapter replaces
//! [`HeadEPathMapAdapter`] after the merge (P12), and the swap touches only this file.
//!
//! The adapter meters nothing: a space operation meters only its COMMs and the bytes that it
//! stores (DR-131).

use std::convert::Infallible;

use models::rhoapi::{EPathMap, Par};
use shared::rust::fallible_sort;

use super::path_codec::{
    bit_sets, entry_key, into_path_elements, plain_list, plain_list_elements, tag,
};
use crate::rust::interpreter::spaces::errors::SpaceError;

/// The mode of a logical PathMap: a set of paths, or a map from paths to values (EPM1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PathMapMode {
    /// A set of paths. Every `EPathMap` of this tree is a set.
    Set,
    /// A map from paths to values, the value-bearing mode of EPM1.
    Map,
}

/// A path of a PathMap value with its entry key. The key is `entry_key(elements)`, so two paths
/// are equal exactly when their keys are equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalPath {
    elements: Vec<Par>,
    key: Vec<u8>,
}

impl LogicalPath {
    /// The path of `elements`. It computes the entry key, so an element that holds a PathMap or a
    /// zipper fails with `UnsupportedPathElement`.
    pub fn new(elements: Vec<Par>) -> Result<Self, SpaceError> {
        let key = entry_key(&elements)?;
        Ok(Self { elements, key })
    }

    /// The elements of the path.
    pub fn elements(&self) -> &[Par] { &self.elements }

    /// The entry key of the path: the segments of the elements and the terminator.
    pub fn key(&self) -> &[u8] { &self.key }

    /// The elements of the path, which the caller now owns.
    pub fn into_elements(self) -> Vec<Par> { self.elements }

    /// The path `prefix ++ self`. Its key is `prefix_key(prefix) ++ key(self)`, so the encode of
    /// the elements of `self` is not made again.
    pub fn prefixed(self, prefix: &LogicalPath) -> Result<Self, SpaceError> {
        let prefix_bytes = match prefix.key.split_last() {
            Some((&tag::TERM, bytes)) => bytes,
            _ => return Err(SpaceError::Inconsistent("an entry key has no terminator")),
        };
        let elements_length = prefix.elements.len() + self.elements.len();
        let key_length = prefix_bytes.len() + self.key.len();
        let mut elements = Vec::<Par>::with_capacity(elements_length);
        for element in &prefix.elements {
            elements.push(element.clone());
        }
        elements.extend(self.elements);
        let mut key = Vec::<u8>::with_capacity(key_length);
        key.extend_from_slice(prefix_bytes);
        key.extend_from_slice(&self.key);
        Ok(Self { elements, key })
    }
}

/// An entry of a logical PathMap: a path, and the value of the path in the map mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalEntry {
    path: LogicalPath,
    value: Option<Par>,
}

impl LogicalEntry {
    /// The path of the entry.
    pub fn path(&self) -> &LogicalPath { &self.path }

    /// The value of the entry in the map mode, or `None` in the set mode.
    pub fn value(&self) -> Option<&Par> { self.value.as_ref() }

    /// The path and the value, which the caller now owns.
    pub fn into_parts(self) -> (LogicalPath, Option<Par>) { (self.path, self.value) }
}

/// A logical PathMap: its mode, and its entries in strictly ascending order of their keys. The
/// set mode holds no values, and the map mode holds one value for each path. The constructors
/// keep these invariants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalPathMap {
    mode: PathMapMode,
    entries: Vec<LogicalEntry>,
}

impl LogicalPathMap {
    /// The set of `paths`, ordered by key. Of the paths with one key, the first in `paths` stays.
    pub fn set(paths: Vec<LogicalPath>) -> Self {
        let mut entries = Vec::<LogicalEntry>::with_capacity(paths.len());
        entries.extend(
            paths
                .into_iter()
                .map(|path| LogicalEntry { path, value: None }),
        );
        Self {
            mode: PathMapMode::Set,
            entries: sorted_unique(entries),
        }
    }

    /// The map of `entries`, ordered by key. Of the entries with one key, the first in `entries`
    /// stays.
    pub fn map(entries: Vec<(LogicalPath, Par)>) -> Self {
        let mut logical = Vec::<LogicalEntry>::with_capacity(entries.len());
        logical.extend(entries.into_iter().map(|(path, value)| LogicalEntry {
            path,
            value: Some(value),
        }));
        Self {
            mode: PathMapMode::Map,
            entries: sorted_unique(logical),
        }
    }

    /// The set that holds only `path`: the unit rule of a re-rooting, `{|s|}`.
    pub fn singleton(path: LogicalPath) -> Self {
        let mut entries = Vec::<LogicalEntry>::with_capacity(1);
        entries.push(LogicalEntry { path, value: None });
        Self {
            mode: PathMapMode::Set,
            entries,
        }
    }

    /// The mode of the map.
    pub fn mode(&self) -> PathMapMode { self.mode }

    /// The entries of the map in strictly ascending key order.
    pub fn entries(&self) -> &[LogicalEntry] { &self.entries }

    /// The number of entries of the map.
    pub fn len(&self) -> usize { self.entries.len() }

    /// True when the map holds no entry.
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// The entries of the map in strictly ascending key order, which the caller now owns.
    pub fn into_entries(self) -> Vec<LogicalEntry> { self.entries }

    /// The map whose entries are the entries of `self` with the path `prefix ++ path`, with the
    /// same mode and the same values. The key of each new path is `prefix_key(prefix) ++ key`,
    /// and a common byte prefix keeps the strict order of the keys (`PathMapPrefix.v`,
    /// `rl_sorted`). So the entries need no new sort.
    pub fn prefixed(self, prefix: &LogicalPath) -> Result<Self, SpaceError> {
        let mut entries = Vec::<LogicalEntry>::with_capacity(self.entries.len());
        for LogicalEntry { path, value } in self.entries {
            entries.push(LogicalEntry {
                path: path.prefixed(prefix)?,
                value,
            });
        }
        Ok(Self {
            mode: self.mode,
            entries,
        })
    }
}

/// The entries in strictly ascending key order, with the first entry of each key. The sort orders
/// by the key and then by the place in `entries`, so the result does not depend on the sort
/// algorithm. The sort is the heap sort of `fallible_sort` with a comparator that cannot fail.
fn sorted_unique(entries: Vec<LogicalEntry>) -> Vec<LogicalEntry> {
    let mut placed = Vec::<(LogicalEntry, usize)>::with_capacity(entries.len());
    placed.extend(
        entries
            .into_iter()
            .enumerate()
            .map(|(place, entry)| (entry, place)),
    );
    let Ok(()) = fallible_sort::sort(&mut placed, |(left, left_place), (right, right_place)| {
        Ok::<_, Infallible>(
            left.path
                .key
                .cmp(&right.path.key)
                .then(left_place.cmp(right_place)),
        )
    });
    let mut unique = Vec::<LogicalEntry>::with_capacity(placed.len());
    for (entry, _) in placed {
        let repeated = match unique.last() {
            Some(last) => last.path.key == entry.path.key,
            None => false,
        };
        if !repeated {
            unique.push(entry);
        }
    }
    unique
}

/// The canonical entry form of the path `elements` in an `EPathMap` of this tree. A path of one
/// element that is not a plain list is the element itself. Every other path is the plain list
/// of its elements. `elems` of the result gives `elements` back for closed elements.
pub fn canonical_entry(mut elements: Vec<Par>) -> Result<Par, SpaceError> {
    let bare = matches!(
        elements.as_slice(),
        [element] if plain_list_elements(element).is_none()
    );
    match bare {
        true => elements
            .pop()
            .ok_or(SpaceError::Inconsistent("a bare entry has no element")),
        false => Ok(plain_list(elements)),
    }
}

/// The adapter between a representation of PathMap values and their logical maps.
pub trait EPathMapAdapter {
    /// The logical map of the PathMap value `map`.
    fn decode(&self, map: EPathMap) -> Result<LogicalPathMap, SpaceError>;

    /// The PathMap value of the logical map `map`.
    fn encode(&self, map: LogicalPathMap) -> Result<EPathMap, SpaceError>;
}

/// The adapter of the `EPathMap` of this tree, which holds the entries in `ps` (set mode only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HeadEPathMapAdapter;

impl EPathMapAdapter for HeadEPathMapAdapter {
    /// The set of the paths of the entries of `map`, ordered and with one entry for each key. A map
    /// with a remainder is a pattern, not a value, so it fails with `Inconsistent`.
    fn decode(&self, map: EPathMap) -> Result<LogicalPathMap, SpaceError> {
        let EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder,
        } = map;
        match remainder {
            Some(_) => Err(SpaceError::Inconsistent("a PathMap value has a remainder")),
            None => {
                let mut paths = Vec::<LogicalPath>::with_capacity(ps.len());
                for entry in ps {
                    paths.push(LogicalPath::new(into_path_elements(entry)?)?);
                }
                Ok(LogicalPathMap::set(paths))
            }
        }
    }

    /// The `EPathMap` of the set `map`: the entries in key order, each in its canonical entry
    /// form. A map in the map mode has no form in this tree, so it fails with `Inconsistent`.
    fn encode(&self, map: LogicalPathMap) -> Result<EPathMap, SpaceError> {
        match map.mode {
            PathMapMode::Map => Err(SpaceError::Inconsistent(
                "the EPathMap of this tree holds no values",
            )),
            PathMapMode::Set => {
                let mut ps = Vec::<Par>::with_capacity(map.entries.len());
                for entry in map.entries {
                    ps.push(canonical_entry(entry.path.into_elements())?);
                }
                let (locally_free, connective_used) = bit_sets(&ps);
                Ok(EPathMap {
                    ps,
                    locally_free,
                    connective_used,
                    remainder: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::Var;

    use super::*;
    use crate::rust::interpreter::spaces::pathmap::path_codec::path_elements;
    use crate::rust::interpreter::spaces::pathmap::test_support::*;

    fn logical(elements: Vec<Par>) -> LogicalPath {
        LogicalPath::new(elements).expect("the path encodes")
    }

    fn decode(entries: Vec<Par>) -> LogicalPathMap {
        HeadEPathMapAdapter
            .decode(epathmap(entries))
            .expect("the map decodes")
    }

    fn encode(map: LogicalPathMap) -> EPathMap {
        HeadEPathMapAdapter.encode(map).expect("the map encodes")
    }

    fn paths(map: &LogicalPathMap) -> Vec<Vec<Par>> {
        map.entries()
            .iter()
            .map(|entry| entry.path().elements().to_vec())
            .collect()
    }

    fn keys(map: &LogicalPathMap) -> Vec<Vec<u8>> {
        map.entries()
            .iter()
            .map(|entry| entry.path().key().to_vec())
            .collect()
    }

    fn strictly_ascending(keys: &[Vec<u8>]) -> bool {
        keys.windows(2).all(|pair| pair[0] < pair[1])
    }

    #[test]
    fn decode_orders_the_paths_and_keeps_the_first_entry_of_each_key() {
        let map = decode(vec![
            gstring("b"),
            gstring("a"),
            glist(vec![gstring("a")]),
            glist(vec![gint(1), gint(2)]),
            gstring("b"),
        ]);
        assert_eq!(map.mode(), PathMapMode::Set);
        assert_eq!(map.len(), 3);
        assert!(!map.is_empty());
        assert_eq!(paths(&map), vec![
            vec![gint(1), gint(2)],
            vec![gstring("a")],
            vec![gstring("b")],
        ]);
        assert_eq!(keys(&map), vec![
            vec![0x03, 0x02, 0x03, 0x04, 0x00],
            vec![0x04, 0x01, b'a', 0x00],
            vec![0x04, 0x01, b'b', 0x00],
        ]);
        assert!(map.entries().iter().all(|entry| entry.value().is_none()));
        assert!(decode(Vec::new()).is_empty());
    }

    #[test]
    fn encode_writes_each_path_in_its_canonical_entry_form() {
        assert_eq!(canonical_entry(Vec::new()), Ok(glist(Vec::new())));
        assert_eq!(canonical_entry(vec![gint(1)]), Ok(gint(1)));
        assert_eq!(
            canonical_entry(vec![glist(vec![gint(1)])]),
            Ok(glist(vec![glist(vec![gint(1)])]))
        );
        assert_eq!(
            canonical_entry(vec![gint(1), gint(2)]),
            Ok(glist(vec![gint(1), gint(2)]))
        );
        // A plain list with an element outside the structural grammar is still a plain list.
        assert_eq!(
            canonical_entry(vec![glist(vec![Par::default()])]),
            Ok(glist(vec![glist(vec![Par::default()])]))
        );
        for path in [
            Vec::new(),
            vec![gint(1)],
            vec![glist(vec![gint(1)])],
            vec![glist(Vec::new())],
            vec![gint(1), gint(2)],
            vec![Par::default()],
            vec![glist(vec![Par::default()])],
            vec![glist(vec![gint(1), send(gint(0), Vec::new())])],
        ] {
            let entry = canonical_entry(path.clone()).expect("the entry builds");
            assert_eq!(path_elements(&entry), path.as_slice(), "{path:?}");
        }
        let encoded = encode(decode(vec![
            gstring("b"),
            glist(vec![gstring("a")]),
            glist(vec![gint(1), gint(2)]),
            glist(vec![glist(Vec::new())]),
        ]));
        assert_eq!(encoded.ps, vec![
            glist(vec![gint(1), gint(2)]),
            gstring("a"),
            gstring("b"),
            glist(vec![glist(Vec::new())]),
        ]);
        assert_eq!(encoded.remainder, None);
        assert!(!encoded.connective_used);
        assert!(encoded.locally_free.is_empty());
    }

    #[test]
    fn encode_folds_the_bit_sets_of_the_entries() {
        let bits = Par {
            locally_free: vec![0b0010],
            connective_used: true,
            ..gint(7)
        };
        let map = LogicalPathMap::set(vec![logical(vec![bits.clone()])]);
        let encoded = encode(map);
        assert_eq!(encoded.ps, vec![bits]);
        assert_eq!(encoded.locally_free, vec![0b0010]);
        assert!(encoded.connective_used);
    }

    #[test]
    fn the_adapter_fails_closed() {
        let pattern = EPathMap {
            remainder: Some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
            connective_used: true,
            ..epathmap(vec![gint(1)])
        };
        assert_eq!(
            HeadEPathMapAdapter.decode(pattern),
            Err(SpaceError::Inconsistent("a PathMap value has a remainder"))
        );
        for entry in [
            pathmap(vec![gint(1)]),
            glist(vec![gint(1), zipper(Vec::new())]),
            send(gint(0), vec![pathmap(Vec::new())]),
        ] {
            assert_eq!(
                HeadEPathMapAdapter.decode(epathmap(vec![gint(2), entry])),
                Err(SpaceError::UnsupportedPathElement)
            );
        }
        let valued = LogicalPathMap::map(vec![(logical(vec![gint(1)]), gint(9))]);
        assert_eq!(
            HeadEPathMapAdapter.encode(valued),
            Err(SpaceError::Inconsistent(
                "the EPathMap of this tree holds no values"
            ))
        );
    }

    #[test]
    fn the_map_mode_keeps_one_value_for_each_path() {
        let map = LogicalPathMap::map(vec![
            (logical(vec![gint(2)]), gstring("second")),
            (logical(vec![gint(1)]), gstring("first")),
            (logical(vec![gint(2)]), gstring("repeat")),
        ]);
        assert_eq!(map.mode(), PathMapMode::Map);
        let values: Vec<Option<&Par>> = map.entries().iter().map(LogicalEntry::value).collect();
        assert_eq!(values, vec![
            Some(&gstring("first")),
            Some(&gstring("second"))
        ]);
        let prefix = logical(vec![gint(0)]);
        let prefixed = map.prefixed(&prefix).expect("the map prefixes");
        assert_eq!(prefixed.mode(), PathMapMode::Map);
        assert_eq!(paths(&prefixed), vec![vec![gint(0), gint(1)], vec![
            gint(0),
            gint(2)
        ]]);
        let (path, value) = prefixed
            .into_entries()
            .into_iter()
            .next()
            .expect("an entry")
            .into_parts();
        assert_eq!(path.into_elements(), vec![gint(0), gint(1)]);
        assert_eq!(value, Some(gstring("first")));
    }

    #[test]
    fn a_prefixed_path_has_the_key_of_the_joined_path() {
        let prefix = logical(vec![gint(0), gstring("x")]);
        let path = logical(vec![glist(vec![gint(1)]), Par::default()]);
        let joined = path.clone().prefixed(&prefix).expect("the path prefixes");
        assert_eq!(
            joined,
            logical(vec![
                gint(0),
                gstring("x"),
                glist(vec![gint(1)]),
                Par::default()
            ])
        );
        assert_eq!(
            logical(Vec::new())
                .prefixed(&prefix)
                .expect("the path prefixes"),
            prefix
        );
        assert_eq!(
            path.clone()
                .prefixed(&logical(Vec::new()))
                .expect("the path prefixes"),
            path
        );
        let singleton = LogicalPathMap::singleton(prefix.clone());
        assert_eq!(singleton.mode(), PathMapMode::Set);
        assert_eq!(paths(&singleton), vec![prefix.elements().to_vec()]);
    }

    // `PathMapPrefix.v`, `head_roundtrip`: the decode of the encode of a logical map is the map.
    // The decode orders the paths and keeps one entry for each path.
    #[test]
    fn prop_head_adapter_roundtrip() {
        check(pure_config(), entries(), |entries| {
            let map = decode(entries.clone());
            ensure(
                strictly_ascending(&keys(&map)),
                "the decode orders the keys",
            )?;
            ensure(
                map.len() <= entries.len(),
                "the decode keeps at most one entry for each entry",
            )?;
            let encoded = encode(map.clone());
            ensure(
                encoded.ps.len() == map.len(),
                "the encode writes one entry for each path",
            )?;
            ensure(decode(encoded.ps) == map, "the decode inverts the encode")?;
            let mut reversed = entries;
            reversed.reverse();
            ensure(
                keys(&decode(reversed)) == keys(&map),
                "the decode does not depend on the order of the entries",
            )
        });
    }

    // `PathMapPrefix.v`, `rl_sorted`: a common prefix keeps the strict order of the keys, and
    // the key of a prefixed path is the key of the joined path.
    #[test]
    fn prop_prefixed_maps_keep_their_order() {
        check(pure_config(), (entries(), path()), |(entries, prefix)| {
            let map = decode(entries);
            let prefix = logical(prefix);
            let prefixed = map.clone().prefixed(&prefix).expect("the map prefixes");
            ensure(
                strictly_ascending(&keys(&prefixed)),
                "the prefixed keys stay in order",
            )?;
            for (entry, original) in prefixed.entries().iter().zip(map.entries()) {
                let joined = [
                    prefix.elements().to_vec(),
                    original.path().elements().to_vec(),
                ]
                .concat();
                ensure(
                    entry.path() == &logical(joined),
                    "a prefixed path has the key of the joined path",
                )?;
            }
            Ok(())
        });
    }
}
