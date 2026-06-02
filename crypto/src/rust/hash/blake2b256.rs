use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

// See crypto/src/main/scala/coop/rchain/crypto/hash/Blake2b256.scala
pub const HASH_LENGTH: usize = 32;

pub struct Blake2b256;

impl Blake2b256 {
    pub fn hash(input: Vec<u8>) -> Vec<u8> {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(input);
        let hash = hasher.finalize();
        hash.to_vec()
    }

    pub fn hash_parts<'a, I>(parts: I) -> Vec<u8>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        let mut hasher = Blake2b::<U32>::new();
        for part in parts {
            hasher.update(part);
        }
        hasher.finalize().to_vec()
    }

    pub fn hash_stream<F>(feed: F) -> Vec<u8>
    where
        F: FnOnce(&mut dyn FnMut(&[u8])),
    {
        let mut hasher = Blake2b::<U32>::new();
        {
            let mut update = |part: &[u8]| hasher.update(part);
            feed(&mut update);
        }
        hasher.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::Blake2b256;

    #[test]
    fn hash_parts_matches_contiguous_input() {
        let parts: [&[u8]; 3] = [b"cost", b"-accounting", b"-trace"];
        let contiguous = parts.concat();

        assert_eq!(Blake2b256::hash(contiguous), Blake2b256::hash_parts(parts));
    }
}
