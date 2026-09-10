use std::collections::{BTreeMap, BTreeSet};

use crypto::rust::hash::blake2b256::Blake2b256;
use proptest::prelude::*;

const GENESIS: u8 = 0;
const FLOOR: u8 = 1;
const SIBLING: u8 = 2;
const COMPATIBLE: u8 = 3;
const INCOMPATIBLE: u8 = 4;
const NODE_COUNT: usize = 3;
const VALIDATOR_COUNT: usize = 3;

type Digest = [u8; 32];

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeploySpec {
    id: Digest,
    payer: u8,
    cost: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BlockSpec {
    id: u8,
    parent: u8,
    declared_floor: u8,
    proposer: usize,
    proposer_generation: u64,
    sequence: u64,
    payload: Vec<u8>,
    deploys: Vec<DeploySpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EconomicEvent {
    event_id: Digest,
    deploy_id: Digest,
    purse: u8,
    amount: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayIdentity {
    block_bytes: Vec<u8>,
    economic_trace: Vec<EconomicEvent>,
    deploy_ids: Vec<Digest>,
    event_ids: Vec<Digest>,
    purse_surface: BTreeMap<u8, i64>,
    committed_effects: BTreeSet<u8>,
    history_root: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EvidenceKey {
    validator: usize,
    generation: u64,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct NodeState {
    online: bool,
    paused: bool,
    cached_roots: BTreeSet<u8>,
    delivered: BTreeSet<u8>,
    accepted: BTreeMap<u8, ReplayIdentity>,
    floor: u8,
    floor_effects: BTreeSet<u8>,
    floor_identity: Option<ReplayIdentity>,
}

impl NodeState {
    fn new() -> Self {
        Self {
            online: true,
            paused: false,
            cached_roots: BTreeSet::from([FLOOR]),
            delivered: BTreeSet::new(),
            accepted: BTreeMap::new(),
            floor: FLOOR,
            floor_effects: BTreeSet::from([GENESIS, FLOOR]),
            floor_identity: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Fixture {
    floor_root: Digest,
    compatible: BlockSpec,
    incompatible: BlockSpec,
}

impl Fixture {
    fn block(&self, id: u8) -> &BlockSpec {
        match id {
            COMPATIBLE => &self.compatible,
            INCOMPATIBLE => &self.incompatible,
            _ => panic!("unknown generated block {id}"),
        }
    }
}

#[derive(Clone, Debug)]
struct Network {
    nodes: Vec<NodeState>,
    support: BTreeMap<u8, BTreeSet<usize>>,
    generations: Vec<u64>,
    evidence: BTreeSet<EvidenceKey>,
}

impl Network {
    fn new() -> Self {
        Self {
            nodes: (0..NODE_COUNT).map(|_| NodeState::new()).collect(),
            support: BTreeMap::new(),
            generations: vec![0; VALIDATOR_COUNT],
            evidence: BTreeSet::new(),
        }
    }

    fn apply(&mut self, fixture: &Fixture, action: &Action) {
        match *action {
            Action::Deliver { node, block } => {
                if self.nodes[node].online {
                    self.nodes[node].delivered.insert(block);
                }
            }
            Action::Pause { node } => self.nodes[node].paused = true,
            Action::Resume { node } => self.nodes[node].paused = false,
            Action::EvictFloorCache { node } => {
                self.nodes[node].cached_roots.remove(&FLOOR);
            }
            Action::RestoreFloorCache { node } => {
                self.nodes[node].cached_roots.insert(FLOOR);
            }
            Action::Crash { node } => {
                self.nodes[node].online = false;
                self.nodes[node].paused = false;
            }
            Action::Restart { node } => self.nodes[node].online = true,
            Action::Retry { node, block } => self.retry(fixture, node, block),
            Action::Support { node, block } => {
                if self.nodes[node].accepted.contains_key(&block) {
                    self.support.entry(block).or_default().insert(node);
                }
            }
            Action::Promote { node, block } => self.promote(fixture, node, block),
            Action::Rebond { validator } => {
                let before = self.evidence.clone();
                self.generations[validator] += 1;
                assert!(before.is_subset(&self.evidence));
            }
            Action::RecordEvidence {
                validator,
                sequence,
            } => {
                self.evidence.insert(EvidenceKey {
                    validator,
                    generation: self.generations[validator],
                    sequence,
                });
            }
        }
        self.assert_invariants(fixture);
    }

    fn retry(&mut self, fixture: &Fixture, node: usize, block: u8) {
        let state = &self.nodes[node];
        if !state.online
            || state.paused
            || !state.delivered.contains(&block)
            || !state.cached_roots.contains(&FLOOR)
            || state.floor != FLOOR
        {
            return;
        }
        let candidate = fixture.block(block);
        if candidate.declared_floor != FLOOR || !descends_from_floor(candidate) {
            return;
        }
        let identity = replay(fixture.floor_root, candidate);
        match self.nodes[node].accepted.get(&block) {
            Some(existing) => assert_eq!(existing, &identity),
            None => {
                self.nodes[node].accepted.insert(block, identity);
            }
        }
    }

    fn promote(&mut self, fixture: &Fixture, node: usize, block: u8) {
        let certified = self
            .support
            .get(&block)
            .is_some_and(|supporters| 2 * supporters.len() > VALIDATOR_COUNT);
        if !self.nodes[node].online || self.nodes[node].paused || !certified {
            return;
        }
        let Some(identity) = self.nodes[node].accepted.get(&block).cloned() else {
            return;
        };
        if self.nodes[node].floor != FLOOR
            || fixture.block(block).declared_floor != FLOOR
            || !self.nodes[node]
                .floor_effects
                .is_subset(&identity.committed_effects)
        {
            return;
        }
        self.nodes[node].floor = block;
        self.nodes[node].floor_effects = identity.committed_effects.clone();
        self.nodes[node].floor_identity = Some(identity);
    }

    fn converge(&mut self, fixture: &Fixture) {
        for node in 0..NODE_COUNT {
            self.apply(fixture, &Action::Restart { node });
            self.apply(fixture, &Action::Resume { node });
            self.apply(fixture, &Action::RestoreFloorCache { node });
            self.apply(fixture, &Action::Deliver {
                node,
                block: COMPATIBLE,
            });
            self.apply(fixture, &Action::Deliver {
                node,
                block: INCOMPATIBLE,
            });
            self.apply(fixture, &Action::Retry {
                node,
                block: COMPATIBLE,
            });
            self.apply(fixture, &Action::Retry {
                node,
                block: INCOMPATIBLE,
            });
            self.apply(fixture, &Action::Support {
                node,
                block: COMPATIBLE,
            });
        }
        for node in 0..NODE_COUNT {
            self.apply(fixture, &Action::Promote {
                node,
                block: COMPATIBLE,
            });
        }
    }

    fn assert_invariants(&self, fixture: &Fixture) {
        for (node_index, node) in self.nodes.iter().enumerate() {
            for (block, identity) in &node.accepted {
                let candidate = fixture.block(*block);
                assert!(descends_from_floor(candidate));
                assert_eq!(candidate.declared_floor, FLOOR);
                assert_eq!(identity, &replay(fixture.floor_root, candidate));
            }
            if node.floor == COMPATIBLE {
                let identity = node
                    .floor_identity
                    .as_ref()
                    .expect("promoted floor has exact replay identity");
                assert_eq!(identity, &replay(fixture.floor_root, &fixture.compatible));
                assert_eq!(&node.floor_effects, &identity.committed_effects);
            } else {
                assert_eq!(node.floor, FLOOR, "node {node_index} promoted a fork");
                assert!(node.floor_identity.is_none());
            }
            assert!(!node.accepted.contains_key(&INCOMPATIBLE));
        }
        for supporters in self.support.values() {
            for supporter in supporters {
                assert!(self.nodes[*supporter].accepted.contains_key(&COMPATIBLE));
            }
        }
        for left in 0..NODE_COUNT {
            for right in 0..NODE_COUNT {
                if self.nodes[left].floor == self.nodes[right].floor {
                    assert_eq!(
                        self.nodes[left].floor_identity,
                        self.nodes[right].floor_identity
                    );
                }
                assert!(
                    self.nodes[left]
                        .floor_effects
                        .is_subset(&self.nodes[right].floor_effects)
                        || self.nodes[right]
                            .floor_effects
                            .is_subset(&self.nodes[left].floor_effects)
                );
            }
        }
        for key in &self.evidence {
            assert!(key.generation <= self.generations[key.validator]);
        }
    }
}

#[derive(Clone, Debug)]
enum Action {
    Deliver { node: usize, block: u8 },
    Pause { node: usize },
    Resume { node: usize },
    EvictFloorCache { node: usize },
    RestoreFloorCache { node: usize },
    Crash { node: usize },
    Restart { node: usize },
    Retry { node: usize, block: u8 },
    Support { node: usize, block: u8 },
    Promote { node: usize, block: u8 },
    Rebond { validator: usize },
    RecordEvidence { validator: usize, sequence: u64 },
}

fn hash(parts: &[&[u8]]) -> Digest {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part);
    }
    Blake2b256::hash(bytes)
        .try_into()
        .expect("Blake2b-256 output has 32 bytes")
}

fn append_bytes(target: &mut Vec<u8>, bytes: &[u8]) {
    target.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    target.extend_from_slice(bytes);
}

fn canonical_block_bytes(block: &BlockSpec) -> Vec<u8> {
    let mut bytes = vec![block.id, block.parent, block.declared_floor];
    bytes.extend_from_slice(&(block.proposer as u64).to_le_bytes());
    bytes.extend_from_slice(&block.proposer_generation.to_le_bytes());
    bytes.extend_from_slice(&block.sequence.to_le_bytes());
    append_bytes(&mut bytes, &block.payload);
    bytes.extend_from_slice(&(block.deploys.len() as u64).to_le_bytes());
    for deploy in &block.deploys {
        bytes.extend_from_slice(&deploy.id);
        bytes.push(deploy.payer);
        bytes.extend_from_slice(&deploy.cost.to_le_bytes());
    }
    bytes
}

fn replay(floor_root: Digest, block: &BlockSpec) -> ReplayIdentity {
    let block_bytes = canonical_block_bytes(block);
    let deploy_ids = block
        .deploys
        .iter()
        .map(|deploy| deploy.id)
        .collect::<Vec<_>>();
    let economic_trace = block
        .deploys
        .iter()
        .enumerate()
        .map(|(index, deploy)| {
            let index_bytes = (index as u64).to_le_bytes();
            let sequence_bytes = block.sequence.to_le_bytes();
            EconomicEvent {
                event_id: hash(&[
                    b"finalization-replay-event",
                    &deploy.id,
                    &sequence_bytes,
                    &index_bytes,
                ]),
                deploy_id: deploy.id,
                purse: deploy.payer,
                amount: deploy.cost,
            }
        })
        .collect::<Vec<_>>();
    let event_ids = economic_trace
        .iter()
        .map(|event| event.event_id)
        .collect::<Vec<_>>();
    let mut purse_surface = (0u8..4)
        .map(|purse| (purse, 1_000i64))
        .collect::<BTreeMap<_, _>>();
    for event in &economic_trace {
        *purse_surface
            .get_mut(&event.purse)
            .expect("generated payer is in the floor surface") -= event.amount as i64;
    }
    let committed_effects = BTreeSet::from([GENESIS, FLOOR, block.id]);
    let mut state_bytes = Vec::new();
    state_bytes.extend_from_slice(&floor_root);
    append_bytes(&mut state_bytes, &block_bytes);
    for event in &economic_trace {
        state_bytes.extend_from_slice(&event.event_id);
        state_bytes.extend_from_slice(&event.deploy_id);
        state_bytes.push(event.purse);
        state_bytes.extend_from_slice(&event.amount.to_le_bytes());
    }
    for (purse, balance) in &purse_surface {
        state_bytes.push(*purse);
        state_bytes.extend_from_slice(&balance.to_le_bytes());
    }
    let history_root = hash(&[b"finalization-replay-root", &state_bytes]);
    ReplayIdentity {
        block_bytes,
        economic_trace,
        deploy_ids,
        event_ids,
        purse_surface,
        committed_effects,
        history_root,
    }
}

fn descends_from_floor(block: &BlockSpec) -> bool { block.id == FLOOR || block.parent == FLOOR }

fn fixture(seed: u64) -> Fixture {
    let floor_root = hash(&[b"finalization-floor", &seed.to_le_bytes()]);
    let deploys = (0..3u64)
        .map(|index| DeploySpec {
            id: hash(&[
                b"finalization-deploy",
                &seed.to_le_bytes(),
                &index.to_le_bytes(),
            ]),
            payer: (seed.wrapping_add(index) % 4) as u8,
            cost: 1 + (seed.rotate_left(index as u32).wrapping_add(index) % 97),
        })
        .collect::<Vec<_>>();
    let compatible = BlockSpec {
        id: COMPATIBLE,
        parent: FLOOR,
        declared_floor: FLOOR,
        proposer: (seed as usize) % VALIDATOR_COUNT,
        proposer_generation: seed % 5,
        sequence: seed,
        payload: seed.to_le_bytes().to_vec(),
        deploys: deploys.clone(),
    };
    let incompatible = BlockSpec {
        id: INCOMPATIBLE,
        parent: SIBLING,
        declared_floor: FLOOR,
        proposer: (seed as usize).wrapping_add(1) % VALIDATOR_COUNT,
        proposer_generation: seed.wrapping_add(1) % 5,
        sequence: seed.saturating_add(1),
        payload: seed.rotate_left(17).to_le_bytes().to_vec(),
        deploys,
    };
    Fixture {
        floor_root,
        compatible,
        incompatible,
    }
}

fn node() -> impl Strategy<Value = usize> { 0usize..NODE_COUNT }

fn validator() -> impl Strategy<Value = usize> { 0usize..VALIDATOR_COUNT }

fn candidate() -> impl Strategy<Value = u8> { prop_oneof![Just(COMPATIBLE), Just(INCOMPATIBLE)] }

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (node(), candidate()).prop_map(|(node, block)| Action::Deliver { node, block }),
        node().prop_map(|node| Action::Pause { node }),
        node().prop_map(|node| Action::Resume { node }),
        node().prop_map(|node| Action::EvictFloorCache { node }),
        node().prop_map(|node| Action::RestoreFloorCache { node }),
        node().prop_map(|node| Action::Crash { node }),
        node().prop_map(|node| Action::Restart { node }),
        (node(), candidate()).prop_map(|(node, block)| Action::Retry { node, block }),
        (node(), candidate()).prop_map(|(node, block)| Action::Support { node, block }),
        (node(), candidate()).prop_map(|(node, block)| Action::Promote { node, block }),
        validator().prop_map(|validator| Action::Rebond { validator }),
        (validator(), 0u64..32).prop_map(|(validator, sequence)| Action::RecordEvidence {
            validator,
            sequence
        }),
    ]
}

#[test]
fn maximum_identity_seed_replays_without_arithmetic_wrap_failure() {
    let fixture = fixture(u64::MAX);
    let first = replay(fixture.floor_root, &fixture.compatible);
    let second = replay(fixture.floor_root, &fixture.compatible);
    assert_eq!(first, second);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 20_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn adversarial_schedules_preserve_exact_replay_and_converge(
        seed in any::<u64>(),
        actions in prop::collection::vec(action(), 0..128),
    ) {
        let fixture = fixture(seed);
        let mut network = Network::new();
        for action in &actions {
            network.apply(&fixture, action);
        }
        network.converge(&fixture);
        network.assert_invariants(&fixture);

        let expected = replay(fixture.floor_root, &fixture.compatible);
        for node in &network.nodes {
            prop_assert_eq!(node.floor, COMPATIBLE);
            prop_assert_eq!(node.floor_identity.as_ref(), Some(&expected));
            prop_assert!(!node.accepted.contains_key(&INCOMPATIBLE));
        }
    }

    #[test]
    fn every_replay_boundary_component_is_consensus_visible(
        seed in any::<u64>(),
        selector in 0u8..7,
    ) {
        let fixture = fixture(seed);
        let original = replay(fixture.floor_root, &fixture.compatible);
        let mut changed = fixture.compatible.clone();
        match selector {
            0 => changed.payload.push(0xff),
            1 => changed.deploys[0].id[0] ^= 1,
            2 => changed.deploys[0].payer = (changed.deploys[0].payer + 1) % 4,
            3 => changed.deploys[0].cost += 1,
            4 => changed.proposer_generation += 1,
            5 => changed.sequence = changed.sequence.wrapping_add(1),
            _ => changed.parent = SIBLING,
        }
        let mutated = replay(fixture.floor_root, &changed);
        prop_assert_ne!(original, mutated);
    }

    #[test]
    fn rebond_keeps_evidence_keys_generation_scoped(
        validator in validator(),
        first_sequences in prop::collection::btree_set(0u64..32, 0..16),
        second_sequences in prop::collection::btree_set(0u64..32, 0..16),
    ) {
        let fixture = fixture(validator as u64);
        let mut network = Network::new();
        for sequence in &first_sequences {
            network.apply(
                &fixture,
                &Action::RecordEvidence {
                    validator,
                    sequence: *sequence,
                },
            );
        }
        network.apply(&fixture, &Action::Rebond { validator });
        for sequence in &second_sequences {
            network.apply(
                &fixture,
                &Action::RecordEvidence {
                    validator,
                    sequence: *sequence,
                },
            );
        }

        let first = network
            .evidence
            .iter()
            .filter(|key| key.validator == validator && key.generation == 0)
            .cloned()
            .collect::<BTreeSet<_>>();
        let second = network
            .evidence
            .iter()
            .filter(|key| key.validator == validator && key.generation == 1)
            .cloned()
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(first.len(), first_sequences.len());
        prop_assert_eq!(second.len(), second_sequences.len());
        prop_assert!(first.is_disjoint(&second));
    }
}
