//! De Bruijn lookup environment.
//!
//! `Env<A>` is a HashMap-backed stack of bindings keyed by absolute
//! position. `put` extends to the right (level+1); `get` resolves a de
//! Bruijn index `k` via `(level + shift) - k - 1` so that the most
//! recently pushed binding is `k=0`. `shift` lets a sub-evaluator see
//! all prior bindings without rebinding indices.

use std::collections::HashMap;
use std::mem::size_of;

use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};
use shared::rust::collection_backing::hash_backing;

#[derive(Clone, Debug)]
pub struct Env<A: Clone> {
    pub env_map: HashMap<i32, A>,
    pub level: i32,
    pub shift: i32,
}

impl<A: Clone> Default for Env<A> {
    fn default() -> Self { Self::new() }
}

impl<A: Clone> Env<A> {
    pub fn new() -> Env<A> {
        Env {
            env_map: HashMap::new(),
            level: 0,
            shift: 0,
        }
    }

    pub fn put(&mut self, a: A) -> Env<A> {
        Env {
            env_map: {
                self.env_map.insert(self.level, a);
                self.env_map.clone()
            },
            level: self.level + 1,
            shift: self.shift,
        }
    }

    pub fn push(&mut self, a: A) -> Result<(), BackingError> {
        let next = self.level.checked_add(1).ok_or(BackingError::Overflow)?;
        self.env_map
            .try_reserve(1)
            .map_err(|_| BackingError::Allocation)?;
        self.env_map.insert(self.level, a);
        self.level = next;
        Ok(())
    }

    pub fn push_metered(&mut self, a: A, meter: &dyn BackingMeter) -> Result<(), BackingError>
    where A: CloneBacking {
        self.level.checked_add(1).ok_or(BackingError::Overflow)?;
        let entries = self
            .env_map
            .len()
            .checked_add(1)
            .ok_or(BackingError::Overflow)?;
        let (operations, backing) =
            hash_backing::<i32, A>(entries).ok_or(BackingError::Overflow)?;
        let scanned = entries
            .checked_mul(size_of::<i32>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(operations, scanned, backing)?;
        self.push(a)
    }

    pub fn get(&self, k: &i32) -> Option<A> {
        let position = self
            .level
            .checked_add(self.shift)?
            .checked_sub(*k)?
            .checked_sub(1)?;
        self.env_map.get(&position).cloned()
    }

    pub fn get_metered(
        &self,
        k: &i32,
        meter: &dyn BackingMeter,
    ) -> Result<Option<A>, BackingError>
    where
        A: CloneBacking,
    {
        let position = self
            .level
            .checked_add(self.shift)
            .and_then(|value| value.checked_sub(*k))
            .and_then(|value| value.checked_sub(1))
            .ok_or(BackingError::Overflow)?;
        let probes = self.env_map.capacity().max(1);
        let scanned = probes
            .checked_mul(size_of::<i32>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(
            probes.checked_add(3).ok_or(BackingError::Overflow)?,
            scanned,
            0,
        )?;
        match self.env_map.get(&position) {
            Some(value) => {
                clone_backing::reserve_copy_and_cleanup(value, meter)?;
                Ok(Some(value.clone()))
            }
            None => Ok(None),
        }
    }

    pub fn shift(&self, j: i32) -> Env<A> {
        Env {
            shift: self.shift + j,
            ..(*self).clone()
        }
    }
}
