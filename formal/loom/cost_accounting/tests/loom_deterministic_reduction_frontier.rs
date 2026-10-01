use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ResourceIntent {
    LeftAb,
    RightBc,
    DisjointZd,
}

fn resource_footprint(intent: ResourceIntent) -> Vec<u8> {
    match intent {
        ResourceIntent::LeftAb => vec![1, 10, 11],
        ResourceIntent::RightBc => vec![2, 11, 12],
        ResourceIntent::DisjointZd => vec![3, 13],
    }
}

fn resource_components(mut intents: Vec<ResourceIntent>) -> Vec<Vec<ResourceIntent>> {
    intents.sort();
    let mut components: Vec<(Vec<u8>, Vec<ResourceIntent>)> = Vec::new();
    for intent in intents {
        let footprint = resource_footprint(intent);
        let overlapping = components
            .iter()
            .enumerate()
            .filter_map(|(index, (existing, _))| {
                existing
                    .iter()
                    .any(|key| footprint.contains(key))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if overlapping.is_empty() {
            components.push((footprint, vec![intent]));
            continue;
        }
        let first = overlapping[0];
        for key in footprint {
            if !components[first].0.contains(&key) {
                components[first].0.push(key);
            }
        }
        components[first].1.push(intent);
        for index in overlapping.into_iter().skip(1).rev() {
            let (keys, mut merged) = components.remove(index);
            for key in keys {
                if !components[first].0.contains(&key) {
                    components[first].0.push(key);
                }
            }
            components[first].1.append(&mut merged);
        }
    }
    components.into_iter().map(|(_, intents)| intents).collect()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Intent {
    ProduceOne,
    ProduceTwo,
    Consume,
}

#[derive(Default)]
struct Frontier {
    active: usize,
    pending: Vec<Intent>,
    data: Vec<usize>,
    output: Option<usize>,
    committed: Vec<Intent>,
    checkpoint: Option<(Vec<usize>, Option<usize>)>,
}

impl Frontier {
    fn with_participants(count: usize) -> Self {
        Self {
            active: count,
            ..Self::default()
        }
    }

    fn submit(&mut self, intent: Intent) {
        self.pending.push(intent);
        if self.pending.len() == self.active {
            self.pending.sort();
            for intent in std::mem::take(&mut self.pending) {
                match intent {
                    Intent::ProduceOne => self.data.push(1),
                    Intent::ProduceTwo => self.data.push(2),
                    Intent::Consume => {
                        self.data.sort();
                        if !self.data.is_empty() {
                            self.output = Some(self.data.remove(0));
                        }
                    }
                }
                self.committed.push(intent);
            }
            self.active = 0;
        }
    }

    fn checkpoint(&mut self) {
        if self.active == 0 && self.pending.is_empty() {
            self.checkpoint = Some((self.data.clone(), self.output));
        }
    }
}

#[test]
fn complete_frontier_has_one_state_for_every_submission_interleaving() {
    loom::model(|| {
        let frontier = Arc::new(Mutex::new(Frontier::with_participants(3)));
        let mut workers = Vec::new();
        for intent in [Intent::ProduceOne, Intent::ProduceTwo, Intent::Consume] {
            let frontier = frontier.clone();
            workers.push(thread::spawn(move || {
                thread::yield_now();
                frontier.lock().unwrap().submit(intent);
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let frontier = frontier.lock().unwrap();
        assert_eq!(frontier.data, vec![2]);
        assert_eq!(frontier.output, Some(1));
        assert_eq!(frontier.committed, vec![
            Intent::ProduceOne,
            Intent::ProduceTwo,
            Intent::Consume
        ]);
    });
}

#[test]
fn checkpoint_never_captures_a_partial_frontier() {
    loom::model(|| {
        let frontier = Arc::new(Mutex::new(Frontier::with_participants(2)));
        let mut workers = Vec::new();
        for intent in [Intent::ProduceOne, Intent::Consume] {
            let frontier = frontier.clone();
            workers.push(thread::spawn(move || {
                frontier.lock().unwrap().submit(intent);
            }));
        }
        let checkpoint = {
            let frontier = frontier.clone();
            thread::spawn(move || {
                thread::yield_now();
                frontier.lock().unwrap().checkpoint();
            })
        };
        for worker in workers {
            worker.join().unwrap();
        }
        checkpoint.join().unwrap();
        let mut frontier = frontier.lock().unwrap();
        frontier.checkpoint();
        assert_eq!(frontier.checkpoint, Some((Vec::new(), Some(1))));
    });
}

#[test]
fn shared_compound_authority_regions_form_one_component_for_every_submission_order() {
    loom::model(|| {
        let submitted = Arc::new(Mutex::new(Vec::new()));
        let mut workers = Vec::new();
        for intent in [
            ResourceIntent::LeftAb,
            ResourceIntent::RightBc,
            ResourceIntent::DisjointZd,
        ] {
            let submitted = submitted.clone();
            workers.push(thread::spawn(move || {
                thread::yield_now();
                submitted.lock().unwrap().push(intent);
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(
            resource_components(submitted.lock().unwrap().clone()),
            vec![
                vec![ResourceIntent::LeftAb, ResourceIntent::RightBc,],
                vec![ResourceIntent::DisjointZd],
            ]
        );
    });
}

#[test]
fn cancelled_parent_aborts_children_before_checkpoint_boundary_opens() {
    loom::model(|| {
        #[derive(Default)]
        struct Epoch {
            root_active: bool,
            child_active: bool,
            child_cancel_requested: bool,
            mutations: usize,
            checkpoint: Option<usize>,
        }
        let epoch = Arc::new(Mutex::new(Epoch {
            root_active: true,
            child_active: true,
            ..Epoch::default()
        }));
        let root = {
            let epoch = epoch.clone();
            thread::spawn(move || {
                let mut epoch = epoch.lock().unwrap();
                epoch.root_active = false;
                if epoch.child_active {
                    epoch.child_cancel_requested = true;
                }
            })
        };
        let child = {
            let epoch = epoch.clone();
            thread::spawn(move || {
                thread::yield_now();
                let mut epoch = epoch.lock().unwrap();
                if !epoch.child_cancel_requested {
                    epoch.mutations += 1;
                }
                epoch.child_active = false;
                epoch.child_cancel_requested = false;
            })
        };
        let checkpoint = {
            let epoch = epoch.clone();
            thread::spawn(move || {
                let mut epoch = epoch.lock().unwrap();
                if !epoch.root_active && !epoch.child_active {
                    epoch.checkpoint = Some(epoch.mutations);
                }
            })
        };
        root.join().unwrap();
        child.join().unwrap();
        checkpoint.join().unwrap();
        let mut epoch = epoch.lock().unwrap();
        if epoch.checkpoint.is_none() {
            epoch.checkpoint = Some(epoch.mutations);
        }
        assert!(!epoch.root_active);
        assert!(!epoch.child_active);
        assert!(!epoch.child_cancel_requested);
        assert_eq!(epoch.checkpoint, Some(epoch.mutations));
        assert!(epoch.mutations <= 1);
    });
}
