use sha2::{Digest, Sha256};

/**
 * Sha256 hashing algorithm
 */
// See crypto/src/main/scala/coop/rchain/crypto/hash/Sha256.scala
pub struct Sha256Hasher;

impl Sha256Hasher {
    pub fn hash(input: Vec<u8>) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_known_answer_for_abc() {
        let result = Sha256Hasher::hash(b"abc".to_vec());
        assert_eq!(
            hex::encode(result),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_matches_known_answer_for_empty_input() {
        let result = Sha256Hasher::hash(Vec::new());
        assert_eq!(
            hex::encode(result),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_output_is_32_bytes_and_deterministic() {
        let a = Sha256Hasher::hash(b"f1r3fly".to_vec());
        let b = Sha256Hasher::hash(b"f1r3fly".to_vec());
        assert_eq!(a.len(), 32);
        assert_eq!(a, b);
        assert_ne!(a, Sha256Hasher::hash(b"f1r3fly!".to_vec()));
    }
}
