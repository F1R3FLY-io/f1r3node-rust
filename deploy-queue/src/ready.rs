use std::collections::{HashMap, VecDeque};

use crate::record::Sig;

/// Deploys ready for release: round-robin across deployers, each deployer's
/// own deploys in intake order. A deployer with many deploys cannot hold back
/// another deployer's single one.
#[derive(Debug, Default)]
pub struct ReadyQueue {
    by_deployer: HashMap<Vec<u8>, VecDeque<Sig>>,
    turn: VecDeque<Vec<u8>>,
    len: usize,
}

impl ReadyQueue {
    pub fn len(&self) -> usize { self.len }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    pub fn push_back(&mut self, deployer: &[u8], sig: Sig) {
        self.deployer_queue(deployer).push_back(sig);
        self.len += 1;
    }

    /// Puts a deploy back at the head of its deployer's queue, for one the
    /// node could not take.
    pub fn push_front(&mut self, deployer: &[u8], sig: Sig) {
        self.deployer_queue(deployer).push_front(sig);
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<Sig> {
        let deployer = self.turn.pop_front()?;
        let queue = self
            .by_deployer
            .get_mut(&deployer)
            .expect("every deployer in the turn order has a queue");
        let sig = queue
            .pop_front()
            .expect("a deployer leaves the turn order when its queue empties");
        if queue.is_empty() {
            self.by_deployer.remove(&deployer);
        } else {
            self.turn.push_back(deployer);
        }
        self.len -= 1;
        Some(sig)
    }

    /// Removes one deploy; false when it is not in the queue.
    pub fn remove(&mut self, deployer: &[u8], sig: &[u8]) -> bool {
        let Some(queue) = self.by_deployer.get_mut(deployer) else {
            return false;
        };
        let Some(position) = queue.iter().position(|held| held == sig) else {
            return false;
        };
        queue.remove(position);
        if queue.is_empty() {
            self.by_deployer.remove(deployer);
            self.turn.retain(|held| held != deployer);
        }
        self.len -= 1;
        true
    }

    pub fn clear(&mut self) {
        self.by_deployer.clear();
        self.turn.clear();
        self.len = 0;
    }

    fn deployer_queue(&mut self, deployer: &[u8]) -> &mut VecDeque<Sig> {
        if !self.by_deployer.contains_key(deployer) {
            self.turn.push_back(deployer.to_vec());
        }
        self.by_deployer.entry(deployer.to_vec()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployers_take_turns_and_keep_their_own_order() {
        let mut ready = ReadyQueue::default();
        for sig in [1u8, 2, 3] {
            ready.push_back(b"a", vec![sig]);
        }
        ready.push_back(b"b", vec![10]);
        ready.push_back(b"c", vec![20]);
        let order: Vec<Sig> = std::iter::from_fn(|| ready.pop()).collect();
        assert_eq!(order, vec![vec![1], vec![10], vec![20], vec![2], vec![3]]);
        assert!(ready.is_empty());
    }

    #[test]
    fn a_returned_deploy_goes_first_for_its_deployer() {
        let mut ready = ReadyQueue::default();
        ready.push_back(b"a", vec![2]);
        ready.push_front(b"a", vec![1]);
        assert_eq!(ready.pop(), Some(vec![1]));
        assert_eq!(ready.pop(), Some(vec![2]));
    }

    #[test]
    fn removing_a_deployers_last_deploy_drops_its_turn() {
        let mut ready = ReadyQueue::default();
        ready.push_back(b"a", vec![1]);
        ready.push_back(b"b", vec![2]);
        assert!(ready.remove(b"a", &[1]));
        assert!(!ready.remove(b"a", &[1]));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready.pop(), Some(vec![2]));
        assert_eq!(ready.pop(), None);
    }
}
