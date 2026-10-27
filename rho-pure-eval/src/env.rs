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
enum Bindings<A: Clone> {
    Dense(Vector<A>),
    Sparse {
        level: i32,
        values: imbl::HashMap<i32, A>,
    },
}

#[derive(Clone, Debug)]
pub struct Env<A: Clone> {
    bindings: Bindings<A>,
    pub shift: i32,
}

impl<A: Clone> Default for Env<A> {
    fn default() -> Self { Self::new() }
}

impl<A: Clone> Env<A> {
    pub fn new() -> Env<A> {
        Env {
            bindings: Bindings::Dense(Vector::new()),
            shift: 0,
        }
    }

    /// The number of bindings pushed so far.
    pub fn level(&self) -> i32 {
        match &self.bindings {
            Bindings::Dense(values) => {
                i32::try_from(values.len()).expect("environment depth fits in i32")
            }
            Bindings::Sparse { level, .. } => *level,
        }
    }

    pub fn put(mut self, a: A) -> Env<A> {
        self.push(a).expect("environment depth fits in i32");
        self
    }

    /// Changed by DR-131 S8: an overflow of the level gives `None`, as the
    /// checked `get` does. An allocation failure aborts, as on dev.
    pub fn push(&mut self, a: A) -> Option<()> {
        let next = self.level().checked_add(1)?;
        match &mut self.bindings {
            Bindings::Dense(values) => values.push_back(a),
            Bindings::Sparse { level, values } => {
                values.insert(*level, a);
                *level = next;
            }
        }
        Some(())
    }

    pub fn from_sparse(level: i32, values: impl IntoIterator<Item = (i32, A)>) -> Self {
        Self {
            bindings: Bindings::Sparse {
                level,
                values: values.into_iter().collect(),
            },
            shift: 0,
        }
    }

    pub fn get_at(&self, position: i32) -> Option<&A> {
        match &self.bindings {
            Bindings::Dense(values) => values.get(usize::try_from(position).ok()?),
            Bindings::Sparse { values, .. } => values.get(&position),
        }
    }

    pub fn pop(&mut self) -> Option<A> {
        match &mut self.bindings {
            Bindings::Dense(values) => values.pop_back(),
            Bindings::Sparse { level, values } => {
                if *level <= 0 {
                    return None;
                }
                *level -= 1;
                values.remove(level)
            }
        }
    }

    pub fn get(&self, k: &i32) -> Option<A> {
        let position = self
            .level()
            .checked_add(self.shift)?
            .checked_sub(*k)?
            .checked_sub(1)?;
        self.get_at(position).cloned()
    }

    pub fn shift(&self, j: i32) -> Env<A> {
        Env {
            bindings: self.bindings.clone(),
            shift: self.shift + j,
        }
    }
}
