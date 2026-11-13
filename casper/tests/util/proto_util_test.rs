// See casper/src/test/scala/coop/rchain/casper/util/ProtoUtilTest.scala

use std::collections::HashSet;

use casper::rust::util::proto_util;
use models::rust::block_implicits::block_element_gen;
use proptest::prelude::*;
use prost::bytes::Bytes;

proptest! {
    #[test]
    fn dependencies_hashes_of_should_return_hashes_of_all_justifications_and_parents_of_a_block(
        block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None)
    ) {
        let result = proto_util::dependencies_hashes_of(&block);

        let justifications_hashes: Vec<Bytes> = block
            .justifications
            .iter()
            .map(|j| j.latest_block_hash.clone())
            .collect();

        let parents_hashes: Vec<Bytes> = block.header.parents_hash_list.clone();

        for hash in &justifications_hashes {
            prop_assert!(result.contains(hash), "Missing justification hash");
        }

        for hash in &parents_hashes {
            prop_assert!(result.contains(hash), "Missing parent hash");
        }

        let result_set: HashSet<Bytes> = result.into_iter().collect();
        let expected: HashSet<Bytes> = justifications_hashes
            .into_iter()
            .chain(parents_hashes.into_iter())
            .collect();

        prop_assert_eq!(result_set, expected);
    }
}
