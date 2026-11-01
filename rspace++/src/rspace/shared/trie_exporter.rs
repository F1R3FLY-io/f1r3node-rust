// See shared/src/main/scala/coop/rchain/state/TrieExporter.scala

use shared::rust::Byte;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rspace::errors::{AuthorityBackendMismatch, RadixTreeError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, ColdLeafStoreError};

// Type of the key to uniquely defines the trie / in RSpace this is the hash of
// the trie
pub type KeyHash = Blake2b256Hash;

// Type of the full path to the node
// - it contains parent nodes with indexes and node itself at the end
pub type NodePath = Vec<(KeyHash, Option<Byte>)>;

pub type Value = Vec<u8>;

// Defines basic operation to traverse tries and convert to path indexed list
pub trait TrieExporter {
    /// Get one authenticated traversal page. Pagination authority is explicit
    /// in the result rather than being smuggled through the last node.
    fn get_page(
        &self,
        start_path: NodePath,
        skip: i32,
        take: i32,
    ) -> Result<AuthenticatedTraversalPage<KeyHash>, StateExportError>;

    // Get history values / from branch nodes in the trie
    fn get_history_items(
        &self,
        keys: Vec<KeyHash>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError>;

    // Get data values / from leaf nodes in the trie
    fn get_data_items(
        &self,
        addresses: Vec<ColdLeafAddress>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateExportError {
    InvalidPath(String),
    MissingHistoryObject(KeyHash),
    CorruptHistoryObject { expected: KeyHash, actual: KeyHash },
    MissingLeafAddress,
    RadixTree(RadixTreeError),
    ColdLeaf(ColdLeafStoreError),
    Store(KvStoreError),
    BackendMismatch(AuthorityBackendMismatch),
}

impl std::fmt::Display for StateExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(error) => write!(f, "invalid state-export path: {error}"),
            Self::MissingHistoryObject(hash) => {
                write!(f, "history object {hash} is missing during state export")
            }
            Self::CorruptHistoryObject { expected, actual } => write!(
                f,
                "history object digest mismatch during state export: expected {expected}, \
                 computed {actual}"
            ),
            Self::MissingLeafAddress => {
                write!(f, "state-export leaf node has no authenticated physical address")
            }
            Self::RadixTree(error) => write!(f, "state-export radix traversal failed: {error}"),
            Self::ColdLeaf(error) => write!(f, "state-export cold leaf failed: {error}"),
            Self::Store(error) => write!(f, "state-export store failed: {error}"),
            Self::BackendMismatch(error) => {
                write!(f, "state-export authority backend mismatch: {error}")
            }
        }
    }
}

impl std::error::Error for StateExportError {}

impl From<RadixTreeError> for StateExportError {
    fn from(error: RadixTreeError) -> Self { Self::RadixTree(error) }
}

impl From<ColdLeafStoreError> for StateExportError {
    fn from(error: ColdLeafStoreError) -> Self { Self::ColdLeaf(error) }
}

impl From<KvStoreError> for StateExportError {
    fn from(error: KvStoreError) -> Self { Self::Store(error) }
}

impl From<AuthorityBackendMismatch> for StateExportError {
    fn from(error: AuthorityBackendMismatch) -> Self { Self::BackendMismatch(error) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrieNode<KeyHash> {
    pub hash: KeyHash,
    pub is_leaf: bool,
    pub leaf_address: Option<ColdLeafAddress>,
}

impl TrieNode<KeyHash> {
    pub fn history(hash: KeyHash) -> Self {
        TrieNode {
            hash,
            is_leaf: false,
            leaf_address: None,
        }
    }

    pub fn leaf(address: ColdLeafAddress) -> Self {
        TrieNode {
            hash: address.digest().clone(),
            is_leaf: true,
            leaf_address: Some(address),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedTraversalPage<KeyHash> {
    pub nodes: Vec<TrieNode<KeyHash>>,
    /// Canonical continuation cursor. A terminal page always repeats its
    /// `start_path`; the empty sentinel never crosses this API boundary.
    pub next_path: Vec<(KeyHash, Option<Byte>)>,
    pub terminal: bool,
}

impl AuthenticatedTraversalPage<KeyHash> {
    pub fn from_raw_cursor(
        start_path: NodePath,
        nodes: Vec<TrieNode<KeyHash>>,
        raw_next_path: NodePath,
    ) -> Result<Self, StateExportError> {
        validate_cursor(&start_path, true)?;
        let Some((start_root, _)) = start_path.first() else {
            return Err(StateExportError::InvalidPath(
                "state-export cursor has no root component".to_string(),
            ));
        };
        let terminal = raw_next_path.is_empty() || raw_next_path == start_path;
        let next_path = if terminal {
            start_path.clone()
        } else {
            validate_cursor(&raw_next_path, false)?;
            let Some((next_root, _)) = raw_next_path.first() else {
                return Err(StateExportError::InvalidPath(
                    "nonterminal state-export cursor has no root component".to_string(),
                ));
            };
            if next_root != start_root {
                return Err(StateExportError::InvalidPath(
                    "state-export continuation changed its root".to_string(),
                ));
            }
            if raw_next_path <= start_path {
                return Err(StateExportError::InvalidPath(
                    "state-export continuation did not advance".to_string(),
                ));
            }
            raw_next_path
        };
        Ok(Self {
            nodes,
            next_path,
            terminal,
        })
    }
}

fn validate_cursor(path: &NodePath, allow_initial: bool) -> Result<(), StateExportError> {
    let valid_length = path.len() == 6 || (allow_initial && path.len() == 1);
    if !valid_length {
        return Err(StateExportError::InvalidPath(format!(
            "state-export cursor must contain {} components, found {}",
            if allow_initial { "one or six" } else { "six" },
            path.len()
        )));
    }
    if path.iter().any(|(component, _)| component.0.len() != 32) {
        return Err(StateExportError::InvalidPath(
            "state-export cursor components must contain 32 bytes".to_string(),
        ));
    }
    if path.iter().any(|(_, index)| index.is_some()) {
        return Err(StateExportError::InvalidPath(
            "canonical state-export cursor components cannot carry child indexes".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod authenticated_page_tests {
    use super::*;

    fn hash(seed: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![seed; 32]) }

    fn continuation(root: Blake2b256Hash, seed: u8) -> NodePath {
        let mut path = vec![(root, None)];
        path.extend((0..5).map(|offset| (hash(seed + offset), None)));
        path
    }

    #[test]
    fn terminal_sentinel_is_normalized_to_the_requested_cursor() {
        let start = continuation(hash(1), 10);
        let page =
            AuthenticatedTraversalPage::from_raw_cursor(start.clone(), Vec::new(), Vec::new())
                .unwrap();
        assert!(page.terminal);
        assert_eq!(page.next_path, start);
    }

    #[test]
    fn continuation_must_be_canonical_root_preserving_and_advancing() {
        let root = hash(1);
        let initial = vec![(root.clone(), None)];
        let next = continuation(root.clone(), 10);
        let page =
            AuthenticatedTraversalPage::from_raw_cursor(initial.clone(), Vec::new(), next.clone())
                .unwrap();
        assert!(!page.terminal);
        assert_eq!(page.next_path, next);

        assert!(
            AuthenticatedTraversalPage::from_raw_cursor(
                initial.clone(),
                Vec::new(),
                continuation(hash(2), 10),
            )
            .is_err()
        );
        assert!(
            AuthenticatedTraversalPage::from_raw_cursor(
                continuation(root.clone(), 20),
                Vec::new(),
                continuation(root, 10),
            )
            .is_err()
        );
        assert!(
            AuthenticatedTraversalPage::from_raw_cursor(initial, Vec::new(), vec![
                (hash(1), None),
                (hash(3), None)
            ],)
            .is_err()
        );
    }
}
