// See rspace/src/main/scala/coop/rchain/rspace/state/RSpaceExporter.scala

use std::sync::Arc;

use shared::rust::{Byte, ByteVector};

use crate::rspace::errors::{AuthorityBackendMismatch, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, ColdLeafStoreError};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::radix_tree::{ExportData, ExportDataSettings, sequential_export};
use crate::rspace::shared::trie_exporter::{
    AuthenticatedTraversalPage, KeyHash, NodePath, StateExportError, TrieExporter, TrieNode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateTransferFormat {
    LegacyRadix,
    DagStreamV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateTransferPage {
    pub bytes: Vec<u8>,
    pub next_cursor: NodePath,
    pub terminal: bool,
}

pub trait RSpaceExporter: TrieExporter + Send + Sync {
    // Get current root
    fn get_root(&self) -> Result<KeyHash, RootError>;

    fn transfer_format(&self) -> StateTransferFormat { StateTransferFormat::LegacyRadix }

    fn get_transfer_page(
        &self,
        _cursor: &[(KeyHash, Option<Byte>)],
    ) -> Result<StateTransferPage, StateExportError> {
        Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::History))
    }
}

pub struct RSpaceExporterInstance;

impl RSpaceExporterInstance {
    pub fn traverse_history_page(
        start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
        skip: i32,
        take: i32,
        get_from_history: Arc<
            dyn Fn(
                &ByteVector,
            )
                -> Result<Option<ByteVector>, crate::rspace::errors::RadixTreeError>,
        >,
    ) -> Result<AuthenticatedTraversalPage<Blake2b256Hash>, StateExportError> {
        let settings = ExportDataSettings {
            flag_node_prefixes: false,
            flag_node_keys: true,
            flag_node_values: false,
            flag_leaf_prefixes: true,
            flag_leaf_values: true,
        };

        fn create_last_prefix(
            prefix_vec: Vec<Blake2b256Hash>,
        ) -> Result<Option<ByteVector>, StateExportError> {
            if prefix_vec.is_empty() {
                Ok(None)
            } else {
                if prefix_vec.len() != 5 {
                    return Err(StateExportError::InvalidPath(format!(
                        "cursor must contain exactly five prefix components, found {}",
                        prefix_vec.len()
                    )));
                }
                if prefix_vec.iter().any(|component| component.0.len() != 32) {
                    return Err(StateExportError::InvalidPath(
                        "cursor prefix components must contain 32 bytes".to_string(),
                    ));
                }

                let (size_prefix, vec): (usize, Vec<Blake2b256Hash>) =
                    (prefix_vec[0].bytes()[0] as usize & 0xff, prefix_vec[1..].to_vec());
                if size_prefix > 127 {
                    return Err(StateExportError::InvalidPath(format!(
                        "cursor prefix length {size_prefix} exceeds 127 bytes"
                    )));
                }
                let mut prefix128 = vec[0].bytes();
                prefix128.extend(vec[1].bytes());
                prefix128.extend(vec[2].bytes());
                prefix128.extend(vec[3].bytes());

                Ok(Some(prefix128.into_iter().take(size_prefix).collect()))
            }
        }

        fn construct_nodes(
            leaf_prefixes: Vec<Vec<u8>>,
            leaf_keys: Vec<Vec<u8>>,
            node_keys: Vec<Vec<u8>>,
        ) -> Result<Vec<TrieNode<Blake2b256Hash>>, StateExportError> {
            if leaf_prefixes.len() != leaf_keys.len() {
                return Err(StateExportError::InvalidPath(format!(
                    "radix traversal returned {} leaf prefixes for {} leaf digests",
                    leaf_prefixes.len(),
                    leaf_keys.len()
                )));
            }
            let mut data_keys: Vec<TrieNode<Blake2b256Hash>> = leaf_keys
                .into_iter()
                .zip(leaf_prefixes)
                .map(|(key, prefix)| {
                    let address = ColdLeafAddress::from_history_prefix(
                        &prefix,
                        Blake2b256Hash::from_bytes(key),
                    )?;
                    Ok(TrieNode::leaf(address))
                })
                .collect::<Result<_, ColdLeafStoreError>>()?;

            let history_keys = node_keys
                .into_iter()
                .map(|key| {
                    if key.len() != 32 {
                        return Err(StateExportError::InvalidPath(format!(
                            "history digest must contain 32 bytes, found {}",
                            key.len()
                        )));
                    }
                    Ok(TrieNode::history(Blake2b256Hash::from_bytes(key)))
                })
                .collect::<Result<Vec<_>, _>>()?;

            data_keys.extend(history_keys);
            Ok(data_keys)
        }

        fn construct_last_path(
            last_prefix: ByteVector,
            root_hash: Blake2b256Hash,
        ) -> Vec<(Blake2b256Hash, Option<Byte>)> {
            let prefix_size = last_prefix.len();
            let mut size_array: Vec<u8> = vec![prefix_size as u8];
            size_array.extend(std::iter::repeat_n(0x00, 31));

            let prefix_zeros: Vec<u8> = std::iter::repeat_n(0x00, 128 - prefix_size).collect();
            let mut prefix128_array = last_prefix.to_vec();
            prefix128_array.extend(prefix_zeros);

            let prefix_blake0 = Blake2b256Hash::from_bytes(size_array);
            let prefix_blake1 = Blake2b256Hash::from_bytes((prefix128_array[0..32]).to_vec());
            let prefix_blake2 = Blake2b256Hash::from_bytes(prefix128_array[32..64].to_vec());
            let prefix_blake3 = Blake2b256Hash::from_bytes(prefix128_array[64..96].to_vec());
            let prefix_blake4 = Blake2b256Hash::from_bytes(prefix128_array[96..128].to_vec());

            vec![
                (root_hash, None),
                (prefix_blake0, None),
                (prefix_blake1, None),
                (prefix_blake2, None),
                (prefix_blake3, None),
                (prefix_blake4, None),
            ]
        }

        if start_path.is_empty() {
            Err(StateExportError::InvalidPath(
                "state-export cursor has no root component".to_string(),
            ))
        } else {
            let requested_start_path = start_path.clone();
            let path_vec: Vec<Blake2b256Hash> =
                start_path.iter().map(|path| path.0.clone()).collect();
            let (root_hash, prefix_vec) = path_vec.split_first().ok_or_else(|| {
                StateExportError::InvalidPath("state-export path has no root".to_string())
            })?;

            let last_prefix = create_last_prefix(prefix_vec.to_vec())?;

            // The canonical empty history root is content-addressed but is not
            // required to be materialized in the history store.  Its complete
            // export is consequently the zero-object terminal page.
            if skip == 0 &&
                prefix_vec.is_empty() &&
                *root_hash == RadixHistory::empty_root_node_hash()
            {
                return AuthenticatedTraversalPage::from_raw_cursor(
                    requested_start_path.clone(),
                    Vec::new(),
                    requested_start_path,
                );
            }
            // println!("\nroot_hash bytes: {:?}", root_hash.bytes());
            // println!("\nlast_prefix: {:?}", last_prefix);

            let exp_res = sequential_export(
                root_hash.bytes(),
                last_prefix,
                skip,
                take,
                get_from_history,
                settings,
            )?;

            // println!("\nexported: {:?}", exp_res);

            let (data, new_last_prefix_opt) = exp_res;
            let ExportData {
                node_prefixes: _,
                node_keys,
                node_values: _,
                leaf_prefixes,
                leaf_values: leaf_keys,
            } = data;

            let nodes = construct_nodes(leaf_prefixes, leaf_keys, node_keys)?;
            let raw_next_path = match new_last_prefix_opt {
                Some(bytes) => construct_last_path(bytes, root_hash.clone()),
                None => Vec::new(),
            };
            AuthenticatedTraversalPage::from_raw_cursor(requested_start_path, nodes, raw_next_path)
        }
    }

    pub fn path_pretty(path: &(Blake2b256Hash, Option<u8>)) -> String {
        let (hash, idx) = path;

        // Format the index as a hexadecimal string or "--" if None
        let idx_str = match idx {
            Some(i) => format!("{:02x}", *i),
            None => "--".to_string(),
        };

        // Format the hash as hex and get the first 8 bytes
        let hash_str = hex::encode(&hash.bytes()[..8]);

        // Return the formatted string
        format!("{}:{}", idx_str, hash_str)
    }
}
