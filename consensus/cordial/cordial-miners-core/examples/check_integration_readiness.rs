use std::collections::{HashMap, HashSet};

use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::validation::{
    ValidationConfig, validate_block, validate_received_block, validated_received_insert,
};
use cordial_miners_core::crypto::{hash_content, sign};
use cordial_miners_core::{Block, BlockContent, BlockIdentity, NodeId};
use k256::ecdsa::SigningKey;

fn signed_initial(seed: u8) -> Block {
    signed_block(seed, HashSet::new())
}

fn signed_block(seed: u8, predecessors: HashSet<BlockIdentity>) -> Block {
    let key = SigningKey::from_slice(&[seed; 32]).unwrap();
    let content = BlockContent {
        payload: b"same initial payload".to_vec(),
        predecessors,
    };
    let content_hash = hash_content(&content);
    Block {
        identity: BlockIdentity {
            content_hash,
            creator: NodeId(key.verifying_key().to_sec1_bytes().to_vec()),
            signature: sign(&content_hash, &key.to_bytes()),
        },
        content,
    }
}

fn main() {
    let first = signed_initial(1);
    let second = signed_initial(2);
    let bonds = HashMap::from([
        (first.identity.creator.clone(), 1),
        (second.identity.creator.clone(), 1),
    ]);
    let empty = Blocklace::new();
    let strict = ValidationConfig::strict();
    assert!(validate_block(&first, &empty, &bonds, &strict).is_valid());
    assert!(validate_block(&second, &empty, &bonds, &strict).is_valid());

    let mut unsigned = first.clone();
    unsigned.identity.signature.clear();
    let unsigned_accepted = validate_block(&unsigned, &empty, &bonds, &strict).is_valid();
    println!("unsigned_block_accepted_by_strict_validation={unsigned_accepted}");

    assert_eq!(first.identity.content_hash, second.identity.content_hash);
    assert_ne!(first.identity, second.identity);
    let mut encodings = HashSet::new();
    let mut hashes = HashSet::new();
    for _ in 0..256 {
        let content = BlockContent {
            payload: b"child".to_vec(),
            predecessors: HashSet::from([first.identity.clone(), second.identity.clone()]),
        };
        encodings.insert(content.predecessors.iter().cloned().collect::<Vec<_>>());
        hashes.insert(hash_content(&content));
    }
    assert_eq!(
        encodings.len(),
        2,
        "the fixture must exercise both set iteration orders"
    );
    let inconsistent_hash = hashes.len() != 1;
    println!("equal_predecessor_sets_produce_different_hashes={inconsistent_hash}");

    let roots: Vec<_> = (1..=4).map(signed_initial).collect();
    let bonds = roots
        .iter()
        .map(|b| (b.identity.creator.clone(), 1))
        .collect();
    let predecessors: HashSet<_> = roots.iter().map(|b| b.identity.clone()).collect();
    let left = signed_block(1, predecessors.clone());
    let right = signed_block(2, predecessors);
    let mut arrival_dependent = false;
    for pair in [[left.clone(), right.clone()], [right, left]] {
        let mut view = Blocklace::new();
        for root in &roots {
            assert!(validated_received_insert(root.clone(), &mut view, &bonds).is_valid());
        }
        assert!(validate_received_block(&pair[0], &view, &bonds).is_valid());
        assert!(validate_received_block(&pair[1], &view, &bonds).is_valid());
        assert!(validated_received_insert(pair[0].clone(), &mut view, &bonds).is_valid());
        arrival_dependent |=
            !validated_received_insert(pair[1].clone(), &mut view, &bonds).is_valid();
    }
    println!("valid_concurrent_block_rejected_after_another_validator_arrives={arrival_dependent}");

    if unsigned_accepted || inconsistent_hash || arrival_dependent {
        eprintln!("Cordial native admission regression gate failed.");
        std::process::exit(1);
    }
    println!(
        "Native admission regression gate passed; production adapter acceptance is still required."
    );
}
