use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::rho_path::{encode_structural_sequence, RhoPathEncode};

use super::canonical_path::encode_trie_path;
use crate::rhoapi::{ListParWithRandom, Par};

impl RhoPathEncode for Par {
    fn rho_path_structure(&self) -> Vec<u8> { encode_trie_path(self) }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}

impl RhoPathEncode for ListParWithRandom {
    fn rho_path_structure(&self) -> Vec<u8> {
        let paths = self.pars.iter().map(encode_trie_path).collect::<Vec<_>>();
        encode_structural_sequence(paths.iter().map(Vec::as_slice))
    }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}
