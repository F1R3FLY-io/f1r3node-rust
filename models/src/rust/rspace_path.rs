use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::rho_path::{RhoPathEncode, StructuralSequenceEncoder};

use super::canonical_path::encode_trie_path;
use super::rholang::match_path::encode_par_match_path;
use crate::rhoapi::{ListParWithRandom, Par};

impl RhoPathEncode for Par {
    fn rho_path_structure(&self) -> Vec<u8> { encode_trie_path(self) }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}

impl RhoPathEncode for ListParWithRandom {
    fn rho_path_structure(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(self.pars.len());
        for par in &self.pars {
            output.push(&encode_par_match_path(par));
        }
        output.finish()
    }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}
