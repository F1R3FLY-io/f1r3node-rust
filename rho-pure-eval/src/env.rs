//! De Bruijn lookup environment.
//!
//! `Env<A>` is a persistent stack of bindings indexed by absolute position.
//! `put` pushes at the next level; `get` resolves a de Bruijn index `k` to
//! position `(level + shift) - k - 1`, so the most recently pushed binding is
//! `k = 0`. `shift` lets a sub-evaluator see all prior bindings without
//! rebinding indices. Clones share structure, so handing an environment to a
//! sub-evaluation does not copy its bindings.

use imbl::Vector;

#[derive(Clone, Debug)]
pub struct Env<A: Clone> {
    bindings: Vector<A>,
    pub shift: i32,
}

impl<A: Clone> Default for Env<A> {
    fn default() -> Self { Self::new() }
}

impl<A: Clone> Env<A> {
    pub fn new() -> Env<A> {
        Env {
            bindings: Vector::new(),
            shift: 0,
        }
    }

    /// The number of bindings pushed so far.
    pub fn level(&self) -> i32 {
        i32::try_from(self.bindings.len()).expect("environment depth fits in i32")
    }

    pub fn put(mut self, a: A) -> Env<A> {
        self.bindings.push_back(a);
        self
    }

    pub fn get(&self, k: &i32) -> Option<A> {
        let position = self.level() + self.shift - k - 1;
        usize::try_from(position)
            .ok()
            .and_then(|position| self.bindings.get(position))
            .cloned()
    }

    pub fn shift(&self, j: i32) -> Env<A> {
        Env {
            bindings: self.bindings.clone(),
            shift: self.shift + j,
        }
    }
}
