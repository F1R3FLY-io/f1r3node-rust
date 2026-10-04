use std::collections::{HashMap, HashSet};

use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::validation::{
    validate_received_block, validated_received_insert,
};
use cordial_miners_core::consensus::{
    InvalidBlock, ValidationConfig, validate_block, validated_insert,
};
use cordial_miners_core::consensus::{
    approves, checkpoint_after_weighted_finality, latest_weighted_final_leader, weighted_tau,
};
use cordial_miners_core::crypto::{Blake2b256Hasher, Hasher, hash_content, sign};
use cordial_miners_core::{Block, BlockContent, BlockIdentity, NodeId};
use k256::ecdsa::SigningKey;

fn signed(seed: u8, payload: Vec<u8>, predecessors: HashSet<BlockIdentity>) -> Block {
    let key = SigningKey::from_slice(&[seed; 32]).unwrap();
    let content = BlockContent {
        payload,
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

#[test]
fn v2_hash_has_a_fixed_vector_and_orders_equal_hash_predecessors() {
    let first = BlockIdentity {
        content_hash: [3; 32],
        creator: NodeId(vec![1]),
        signature: vec![7],
    };
    let second = BlockIdentity {
        content_hash: [3; 32],
        creator: NodeId(vec![2]),
        signature: vec![9],
    };
    let expected = [
        0x15, 0x01, 0x97, 0x24, 0x65, 0xed, 0x19, 0xe0, 0xa2, 0xcf, 0x24, 0xcb, 0x57, 0xdd, 0x48,
        0x2d, 0x58, 0x4a, 0xb9, 0xd1, 0xd3, 0x97, 0xaa, 0xf4, 0x7f, 0xe9, 0xb7, 0x28, 0x14, 0x68,
        0x02, 0x4c,
    ];
    for _ in 0..128 {
        let content = BlockContent {
            payload: vec![42],
            predecessors: HashSet::from([first.clone(), second.clone()]),
        };
        assert_eq!(hash_content(&content), expected);
    }
}

#[test]
fn strict_admission_rejects_a_missing_signature_without_mutation() {
    let mut block = signed(1, vec![1], HashSet::new());
    let bonds = HashMap::from([(block.identity.creator.clone(), 1)]);
    assert!(
        validate_block(
            &block,
            &Blocklace::new(),
            &bonds,
            &ValidationConfig::strict()
        )
        .is_valid()
    );
    block.identity.signature.clear();
    let mut view = Blocklace::new();
    let result = validated_insert(block, &mut view, &bonds, &ValidationConfig::strict());
    assert!(result.errors().contains(&InvalidBlock::InvalidSignature));
    assert!(view.dom().is_empty());
}

#[test]
fn received_concurrent_blocks_do_not_depend_on_arrival_order() {
    let roots: Vec<_> = (1..=4)
        .map(|seed| signed(seed, vec![0], HashSet::new()))
        .collect();
    let bonds = roots
        .iter()
        .map(|b| (b.identity.creator.clone(), 1))
        .collect();
    let predecessors: HashSet<_> = roots.iter().map(|b| b.identity.clone()).collect();
    let left = signed(1, vec![1], predecessors.clone());
    let right = signed(2, vec![2], predecessors);
    for pair in [[left.clone(), right.clone()], [right.clone(), left.clone()]] {
        let mut view = Blocklace::new();
        for root in &roots {
            assert!(validated_received_insert(root.clone(), &mut view, &bonds).is_valid());
        }
        assert!(validate_received_block(&pair[0], &view, &bonds).is_valid());
        assert!(validated_received_insert(pair[0].clone(), &mut view, &bonds).is_valid());
        assert!(!validate_block(&pair[1], &view, &bonds, &ValidationConfig::strict()).is_valid());
        assert!(validate_received_block(&pair[1], &view, &bonds).is_valid());
        assert!(validated_received_insert(pair[1].clone(), &mut view, &bonds).is_valid());
        let generation = view.generation();
        assert!(validated_received_insert(pair[1].clone(), &mut view, &bonds).is_valid());
        assert_eq!(view.generation(), generation);
    }
}

fn initial_view() -> (Blocklace, HashMap<NodeId, u64>, Vec<Block>) {
    let roots: Vec<_> = (1..=4)
        .map(|seed| signed(seed, vec![0], HashSet::new()))
        .collect();
    let bonds = roots
        .iter()
        .map(|b| (b.identity.creator.clone(), 1))
        .collect();
    let mut view = Blocklace::new();
    for root in &roots {
        assert!(validated_received_insert(root.clone(), &mut view, &bonds).is_valid());
    }
    (view, bonds, roots)
}

#[test]
fn received_admission_rejects_tampering_unknown_and_zero_weight_senders() {
    let block = signed(1, vec![1], HashSet::new());
    let bonds = HashMap::from([(block.identity.creator.clone(), 1)]);
    let mut cases = Vec::new();
    let mut unsigned = block.clone();
    unsigned.identity.signature.clear();
    cases.push((unsigned, bonds.clone()));
    let mut bad_signature = block.clone();
    bad_signature.identity.signature[0] ^= 1;
    cases.push((bad_signature, bonds.clone()));
    let mut bad_content = block.clone();
    bad_content.content.payload.push(2);
    cases.push((bad_content, bonds.clone()));
    let mut bad_hash = block.clone();
    bad_hash.identity.content_hash[0] ^= 1;
    cases.push((bad_hash, bonds.clone()));
    cases.push((block.clone(), HashMap::new()));
    cases.push((
        block.clone(),
        HashMap::from([(block.identity.creator.clone(), 0)]),
    ));
    for (candidate, membership) in cases {
        let mut view = Blocklace::new();
        assert!(!validated_received_insert(candidate, &mut view, &membership).is_valid());
        assert_eq!(view.generation(), 0);
        assert!(view.dom().is_empty());
    }
}

#[test]
fn correctly_signed_legacy_content_is_not_silently_accepted_as_v2() {
    let mut block = signed(1, vec![1], HashSet::new());
    let mut legacy_bytes = 1u64.to_le_bytes().to_vec();
    legacy_bytes.push(1);
    legacy_bytes.extend_from_slice(&0u64.to_le_bytes());
    block.identity.content_hash = Blake2b256Hasher.hash(&legacy_bytes);
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    block.identity.signature = sign(&block.identity.content_hash, &key.to_bytes());
    let bonds = HashMap::from([(block.identity.creator.clone(), 1)]);
    let result = validate_received_block(&block, &Blocklace::new(), &bonds);
    assert!(matches!(
        result.errors(),
        [InvalidBlock::InvalidContentHash { .. }]
    ));
}

#[test]
fn previous_round_support_requires_strict_weighted_supermajority() {
    let (mut view, bonds, roots) = initial_view();
    let generation = view.generation();
    let insufficient = signed(
        1,
        vec![1],
        roots[..2].iter().map(|b| b.identity.clone()).collect(),
    );
    assert_eq!(
        validated_received_insert(insufficient, &mut view, &bonds).errors(),
        &[InvalidBlock::InsufficientRoundSupport { round: 0 }]
    );
    assert_eq!(view.generation(), generation);
    let mut three = bonds.clone();
    three.remove(&roots[3].identity.creator);
    let exactly_two_thirds = signed(
        1,
        vec![1],
        roots[..2].iter().map(|b| b.identity.clone()).collect(),
    );
    assert_eq!(
        validate_received_block(&exactly_two_thirds, &view, &three).errors(),
        &[InvalidBlock::InsufficientRoundSupport { round: 0 }]
    );
    let sufficient = signed(
        1,
        vec![1],
        roots[..3].iter().map(|b| b.identity.clone()).collect(),
    );
    assert!(validated_received_insert(sufficient.clone(), &mut view, &bonds).is_valid());
    let mut mixed: HashSet<_> = roots.iter().map(|b| b.identity.clone()).collect();
    mixed.insert(sufficient.identity);
    let only_one_at_previous_round = signed(2, vec![2], mixed);
    assert_eq!(
        validate_received_block(&only_one_at_previous_round, &view, &bonds).errors(),
        &[InvalidBlock::InsufficientRoundSupport { round: 1 }]
    );
}

#[test]
fn missing_dependencies_defer_without_mutation_and_retry_succeeds() {
    let (complete, bonds, roots) = initial_view();
    let child = signed(
        1,
        vec![1],
        roots.iter().map(|b| b.identity.clone()).collect(),
    );
    let mut view = Blocklace::new();
    for root in &roots[..3] {
        assert!(validated_received_insert(root.clone(), &mut view, &bonds).is_valid());
    }
    let generation = view.generation();
    assert_eq!(
        validated_received_insert(child.clone(), &mut view, &bonds).errors(),
        &[InvalidBlock::MissingPredecessors {
            missing: vec![roots[3].identity.clone()]
        }]
    );
    assert_eq!(view.generation(), generation);
    assert!(validated_received_insert(roots[3].clone(), &mut view, &bonds).is_valid());
    assert_eq!(view.dom(), complete.dom());
    assert!(validated_received_insert(child, &mut view, &bonds).is_valid());
}

#[test]
fn equivocation_is_retained_but_does_not_count_twice_or_approve_either_branch() {
    let (_, bonds, roots) = initial_view();
    let predecessors: HashSet<_> = roots.iter().map(|b| b.identity.clone()).collect();
    let left = signed(1, vec![1], predecessors.clone());
    let right = signed(1, vec![2], predecessors.clone());
    for pair in [[left.clone(), right.clone()], [right.clone(), left.clone()]] {
        let (mut view, _, _) = initial_view();
        for block in pair {
            assert!(validated_received_insert(block, &mut view, &bonds).is_valid());
        }
        let peer = signed(2, vec![3], predecessors.clone());
        let peer2 = signed(3, vec![4], predecessors.clone());
        assert!(validated_received_insert(peer.clone(), &mut view, &bonds).is_valid());
        let false_quorum = signed(
            4,
            vec![5],
            HashSet::from([
                left.identity.clone(),
                right.identity.clone(),
                peer.identity.clone(),
            ]),
        );
        assert_eq!(
            validate_received_block(&false_quorum, &view, &bonds).errors(),
            &[InvalidBlock::InsufficientRoundSupport { round: 1 }]
        );
        assert!(validated_received_insert(peer2.clone(), &mut view, &bonds).is_valid());
        let next = HashSet::from([
            left.identity.clone(),
            right.identity.clone(),
            peer.identity,
            peer2.identity,
        ]);
        let self_conflict = signed(1, vec![6], next.clone());
        assert!(
            validate_received_block(&self_conflict, &view, &bonds)
                .errors()
                .iter()
                .any(|error| matches!(error, InvalidBlock::Equivocation { .. }))
        );
        let honest = signed(4, vec![7], next);
        assert!(validated_received_insert(honest.clone(), &mut view, &bonds).is_valid());
        assert!(!approves(&view, &honest.identity, &left.identity));
        assert!(!approves(&view, &honest.identity, &right.identity));
    }
}

#[test]
fn signed_multi_wave_delivery_preserves_native_finality_and_ordered_prefix() {
    let (mut forward, bonds, roots) = initial_view();
    let (mut reversed, _, _) = initial_view();
    let leader_id = roots[0].identity.creator.clone();
    let leader = |_| Some(leader_id.clone());
    let mut previous = roots;
    let mut prefix = Vec::new();
    for round in 1..=11 {
        let predecessors: HashSet<_> = previous.iter().map(|b| b.identity.clone()).collect();
        let blocks: Vec<_> = (1..=4)
            .map(|seed| signed(seed, vec![round], predecessors.clone()))
            .collect();
        for block in &blocks {
            assert!(validated_received_insert(block.clone(), &mut forward, &bonds).is_valid());
        }
        for block in blocks.iter().rev() {
            assert!(validated_received_insert(block.clone(), &mut reversed, &bonds).is_valid());
        }
        assert_eq!(
            latest_weighted_final_leader(&forward, 3, &bonds, leader),
            latest_weighted_final_leader(&reversed, 3, &bonds, leader)
        );
        let output = weighted_tau(&forward, 3, &bonds, leader).unwrap();
        assert_eq!(output, weighted_tau(&reversed, 3, &bonds, leader).unwrap());
        assert!(output.starts_with(&prefix));
        prefix = output;
        previous = blocks;
    }
    assert!(latest_weighted_final_leader(&forward, 3, &bonds, leader).is_some());
    assert!(prefix.len() >= 25, "the test must commit multiple waves");
    assert!(
        checkpoint_after_weighted_finality(&mut forward, 3, &bonds, leader)
            .unwrap()
            .is_some()
    );
    let generation = forward.generation();
    let next = signed(
        1,
        vec![12],
        previous.iter().map(|b| b.identity.clone()).collect(),
    );
    assert_eq!(
        validated_received_insert(next, &mut forward, &bonds).errors(),
        &[InvalidBlock::UnsupportedPrunedHistory]
    );
    assert_eq!(forward.generation(), generation);
}
