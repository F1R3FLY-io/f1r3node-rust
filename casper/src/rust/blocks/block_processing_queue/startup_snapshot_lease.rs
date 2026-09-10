#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseRole {
    Pending,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeasePhase {
    Reserved,
    Capturing,
    Built,
    Stored,
    Active,
    Retiring,
    Destroyed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotLease<K, I> {
    pub key: K,
    pub identity: I,
    pub phase: LeasePhase,
}

#[derive(Debug)]
pub struct SnapshotLeases<K, I> {
    pending: Option<SnapshotLease<K, I>>,
    active: Option<SnapshotLease<K, I>>,
}

impl<K, I> Default for SnapshotLeases<K, I> {
    fn default() -> Self {
        Self {
            pending: None,
            active: None,
        }
    }
}

impl<K: Eq, I: Clone + Eq> SnapshotLeases<K, I> {
    pub fn pending(&self) -> Option<&SnapshotLease<K, I>> { self.pending.as_ref() }

    pub fn active(&self) -> Option<&SnapshotLease<K, I>> { self.active.as_ref() }

    pub fn reserve(&mut self, key: K, identity: I) -> bool {
        if self.pending.is_some()
            || self
                .active
                .as_ref()
                .is_some_and(|entry| entry.identity == identity)
        {
            return false;
        }
        self.pending = Some(SnapshotLease {
            key,
            identity,
            phase: LeasePhase::Reserved,
        });
        true
    }

    pub fn begin_capture(&mut self, identity: &I) -> bool {
        self.advance(
            LeaseRole::Pending,
            identity,
            LeasePhase::Reserved,
            LeasePhase::Capturing,
        )
    }

    pub fn finish_capture(&mut self, identity: &I) -> bool {
        self.advance(
            LeaseRole::Pending,
            identity,
            LeasePhase::Capturing,
            LeasePhase::Built,
        )
    }

    pub fn store(&mut self, key: &K, identity: &I) -> bool {
        if self.pending.as_ref().is_none_or(|entry| &entry.key != key) {
            return false;
        }
        self.advance(
            LeaseRole::Pending,
            identity,
            LeasePhase::Built,
            LeasePhase::Stored,
        )
    }

    pub fn activate(&mut self, key: &K) -> Option<I> {
        if self.active.is_some()
            || self
                .pending
                .as_ref()
                .is_none_or(|entry| &entry.key != key || entry.phase != LeasePhase::Stored)
        {
            return None;
        }
        let mut entry = self.pending.take()?;
        entry.phase = LeasePhase::Active;
        let identity = entry.identity.clone();
        self.active = Some(entry);
        Some(identity)
    }

    pub fn retire_stored(&mut self, identity: &I) -> bool {
        self.advance(
            LeaseRole::Pending,
            identity,
            LeasePhase::Stored,
            LeasePhase::Retiring,
        )
    }

    pub fn abandon_capture(&mut self, identity: &I) -> bool {
        let Some(entry) = self.pending.as_ref() else {
            return false;
        };
        if !matches!(
            entry.phase,
            LeasePhase::Reserved | LeasePhase::Capturing | LeasePhase::Built
        ) {
            return false;
        }
        self.advance(
            LeaseRole::Pending,
            identity,
            entry.phase,
            LeasePhase::Retiring,
        )
    }

    pub fn retire_active(&mut self, identity: &I) -> bool {
        self.advance(
            LeaseRole::Active,
            identity,
            LeasePhase::Active,
            LeasePhase::Retiring,
        )
    }

    pub fn destroyed(&mut self, role: LeaseRole, identity: &I) -> bool {
        self.advance(role, identity, LeasePhase::Retiring, LeasePhase::Destroyed)
    }

    pub fn release(&mut self, role: LeaseRole, identity: &I) -> bool {
        let slot = match role {
            LeaseRole::Pending => &mut self.pending,
            LeaseRole::Active => &mut self.active,
        };
        if slot
            .as_ref()
            .is_none_or(|entry| &entry.identity != identity || entry.phase != LeasePhase::Destroyed)
        {
            return false;
        }
        *slot = None;
        true
    }

    fn advance(
        &mut self,
        role: LeaseRole,
        identity: &I,
        expected: LeasePhase,
        next: LeasePhase,
    ) -> bool {
        let slot = match role {
            LeaseRole::Pending => &mut self.pending,
            LeaseRole::Active => &mut self.active,
        };
        let Some(entry) = slot.as_mut() else {
            return false;
        };
        if &entry.identity != identity || entry.phase != expected {
            return false;
        }
        entry.phase = next;
        true
    }
}
