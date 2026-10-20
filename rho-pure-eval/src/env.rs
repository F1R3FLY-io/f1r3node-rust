//! De Bruijn lookup environment.
//!
//! `Env<A>` is a HashMap-backed stack of bindings keyed by absolute
//! position. `put` extends to the right (level+1); `get` resolves a de
//! Bruijn index `k` via `(level + shift) - k - 1` so that the most
//! recently pushed binding is `k=0`. `shift` lets a sub-evaluator see
//! all prior bindings without rebinding indices.

use std::collections::HashMap;

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

    /// Changed by DR-131 S8: an overflow of the level gives `None`, as the
    /// checked `get` does. An allocation failure aborts, as on dev.
    pub fn push(&mut self, a: A) -> Option<()> {
        let next = self.level.checked_add(1)?;
        self.env_map.insert(self.level, a);
        self.level = next;
        Some(())
    }

    pub fn get(&self, k: &i32) -> Option<A> {
        let position = self
            .level
            .checked_add(self.shift)?
            .checked_sub(*k)?
            .checked_sub(1)?;
        self.env_map.get(&position).cloned()
    }

    /// Borrow the binding for de Bruijn index `k` without cloning — the same
    /// index resolution as [`Env::get`], minus its deep clone of the bound
    /// value.
    ///
    /// Additive: the interpreter's EPathMap method-chain fusion
    /// recognizer inspects a chain's base binding BY BORROW to decide whether
    /// the chain fuses; `get`'s clone is exactly the per-reference
    /// materialization the fusion removes, so the recognizer must not pay it
    /// just to look. All existing `get` call sites are unchanged.
    pub fn get_ref(&self, k: &i32) -> Option<&A> {
        self.env_map.get(&((self.level + self.shift) - k - 1))
    }

    pub fn shift(&self, j: i32) -> Env<A> {
        Env {
            shift: self.shift + j,
            ..(*self).clone()
        }
    }
}
