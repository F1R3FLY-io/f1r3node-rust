use vstd::prelude::*;

verus! {

pub struct FreeMapCheckpoint {
    pub index: usize,
}

pub struct BindingDelta {
    pub changed: bool,
    pub value: Option<u64>,
}

impl BindingDelta {
    pub fn len(&self) -> (result: usize)
        ensures result == if self.changed { 1usize } else { 0usize },
    {
        if self.changed { 1 } else { 0 }
    }
}

pub struct FreeMapTransaction {
    pub root: Option<u64>,
    pub current: Option<u64>,
    pub undo: Vec<Option<u64>>,
    pub finished: bool,
}

impl FreeMapTransaction {
    pub open spec fn valid(&self) -> bool {
        !self.finished ==> if self.undo@.len() == 0 {
            self.current == self.root
        } else {
            self.undo@[0] == self.root
        }
    }
}

pub open spec fn state_at(
    history: Seq<Option<u64>>,
    current: Option<u64>,
    checkpoint: int,
) -> Option<u64>
    recommends 0 <= checkpoint <= history.len(),
{
    if checkpoint == history.len() {
        current
    } else {
        history[checkpoint]
    }
}

impl FreeMapTransaction {
    pub fn new(initial: Option<u64>) -> (result: Self)
        ensures
            result.root == initial,
            result.current == initial,
            result.undo@.len() == 0,
            !result.finished,
            result.valid(),
    {
        Self {
            root: initial,
            current: initial,
            undo: Vec::new(),
            finished: false,
        }
    }

    pub fn checkpoint(&self) -> (result: FreeMapCheckpoint)
        requires !self.finished, self.valid(),
        ensures result.index == self.undo@.len(),
    {
        FreeMapCheckpoint {
            index: self.undo.len(),
        }
    }

    pub fn write(&mut self, value: u64)
        requires !old(self).finished, old(self).valid(),
        ensures
            final(self).root == old(self).root,
            final(self).current == Some(value),
            final(self).undo@ == old(self).undo@.push(old(self).current),
            final(self).undo@.len() == old(self).undo@.len() + 1,
            !final(self).finished,
            final(self).valid(),
    {
        let previous = self.current;
        self.undo.push(previous);
        self.current = Some(value);
    }

    pub fn rollback(&mut self, checkpoint: FreeMapCheckpoint)
        requires
            !old(self).finished,
            old(self).valid(),
            checkpoint.index <= old(self).undo@.len(),
        ensures
            final(self).root == old(self).root,
            final(self).current == state_at(
                old(self).undo@,
                old(self).current,
                checkpoint.index as int,
            ),
            final(self).undo@ == old(self).undo@.take(checkpoint.index as int),
            final(self).undo@.len() == checkpoint.index,
            !final(self).finished,
            final(self).valid(),
    {
        let baseline = if checkpoint.index == self.undo.len() {
            self.current
        } else {
            self.undo[checkpoint.index]
        };
        self.undo.truncate(checkpoint.index);
        self.current = baseline;
    }

    pub fn take_delta_and_rollback(
        &mut self,
        checkpoint: FreeMapCheckpoint,
    ) -> (result: BindingDelta)
        requires
            !old(self).finished,
            old(self).valid(),
            checkpoint.index <= old(self).undo@.len(),
        ensures
            final(self).root == old(self).root,
            final(self).current == state_at(
                old(self).undo@,
                old(self).current,
                checkpoint.index as int,
            ),
            final(self).undo@ == old(self).undo@.take(checkpoint.index as int),
            final(self).undo@.len() == checkpoint.index,
            result.value == old(self).current,
            result.changed == (
                old(self).current != state_at(
                    old(self).undo@,
                    old(self).current,
                    checkpoint.index as int,
                )
            ),
            result.changed ==> checkpoint.index < old(self).undo@.len(),
            (if result.changed { 1int } else { 0int })
                <= old(self).undo@.len() - checkpoint.index,
            !final(self).finished,
            final(self).valid(),
    {
        let final_value = self.current;
        let baseline = if checkpoint.index == self.undo.len() {
            self.current
        } else {
            self.undo[checkpoint.index]
        };
        let changed = final_value != baseline;
        self.rollback(checkpoint);
        BindingDelta {
            changed,
            value: final_value,
        }
    }

    pub fn abort(&mut self)
        requires !old(self).finished, old(self).valid(),
        ensures
            final(self).root == old(self).root,
            final(self).current == old(self).root,
            final(self).undo@.len() == 0,
            final(self).finished,
            final(self).valid(),
    {
        self.rollback(FreeMapCheckpoint { index: 0 });
        self.finished = true;
    }

    pub fn finish(&mut self, matched: bool) -> (result: bool)
        requires !old(self).finished, old(self).valid(),
        ensures
            result == matched,
            final(self).root == old(self).root,
            final(self).current == if matched { old(self).current } else { old(self).root },
            final(self).undo@.len() == 0,
            final(self).finished,
            final(self).valid(),
    {
        if matched {
            self.undo.clear();
            self.finished = true;
        } else {
            self.abort();
        }
        matched
    }
}

pub fn exec_write_refines_spec(transaction: &mut FreeMapTransaction, value: u64)
    requires
        !old(transaction).finished,
        old(transaction).valid(),
    ensures
        final(transaction).root == old(transaction).root,
        final(transaction).current == Some(value),
        final(transaction).undo@ == old(transaction).undo@.push(old(transaction).current),
        final(transaction).undo@.len() == old(transaction).undo@.len() + 1,
        !final(transaction).finished,
        final(transaction).valid(),
{
    transaction.write(value);
}

pub fn exec_rollback_refines_spec(
    transaction: &mut FreeMapTransaction,
    checkpoint: FreeMapCheckpoint,
)
    requires
        !old(transaction).finished,
        old(transaction).valid(),
        checkpoint.index <= old(transaction).undo@.len(),
    ensures
        final(transaction).root == old(transaction).root,
        final(transaction).current == state_at(
            old(transaction).undo@,
            old(transaction).current,
            checkpoint.index as int,
        ),
        final(transaction).undo@ == old(transaction).undo@.take(checkpoint.index as int),
        final(transaction).undo@.len() == checkpoint.index,
        !final(transaction).finished,
        final(transaction).valid(),
{
    transaction.rollback(checkpoint);
}

pub fn exec_take_delta_and_rollback_refines_spec(
    transaction: &mut FreeMapTransaction,
    checkpoint: FreeMapCheckpoint,
) -> (result: BindingDelta)
    requires
        !old(transaction).finished,
        old(transaction).valid(),
        checkpoint.index <= old(transaction).undo@.len(),
    ensures
        final(transaction).root == old(transaction).root,
        final(transaction).current == state_at(
            old(transaction).undo@,
            old(transaction).current,
            checkpoint.index as int,
        ),
        final(transaction).undo@ == old(transaction).undo@.take(checkpoint.index as int),
        result.value == old(transaction).current,
        result.changed == (
            old(transaction).current != state_at(
                old(transaction).undo@,
                old(transaction).current,
                checkpoint.index as int,
            )
        ),
        (if result.changed { 1int } else { 0int })
            <= old(transaction).undo@.len() - checkpoint.index,
        !final(transaction).finished,
        final(transaction).valid(),
{
    transaction.take_delta_and_rollback(checkpoint)
}

pub fn exec_finish_refines_spec(
    transaction: &mut FreeMapTransaction,
    matched: bool,
) -> (result: bool)
    requires
        !old(transaction).finished,
        old(transaction).valid(),
    ensures
        result == matched,
        final(transaction).root == old(transaction).root,
        final(transaction).current == if matched {
            old(transaction).current
        } else {
            old(transaction).root
        },
        final(transaction).undo@.len() == 0,
        final(transaction).finished,
        final(transaction).valid(),
{
    transaction.finish(matched)
}

pub fn abort_guard_restores_root_spec(transaction: &mut FreeMapTransaction)
    requires
        !old(transaction).finished,
        old(transaction).valid(),
    ensures
        final(transaction).root == old(transaction).root,
        final(transaction).current == old(transaction).root,
        final(transaction).undo@.len() == 0,
        final(transaction).finished,
        final(transaction).valid(),
{
    transaction.abort();
}

pub open spec fn undo_len_after_completed_writes(
    initial_len: nat,
    completed_writes: nat,
) -> nat {
    initial_len + completed_writes
}

pub proof fn undo_len_tracks_completed_writes(initial_len: nat, completed_writes: nat)
    ensures
        undo_len_after_completed_writes(initial_len, completed_writes)
            == initial_len + completed_writes,
        undo_len_after_completed_writes(initial_len, completed_writes) >= completed_writes,
{
}

pub proof fn delta_len_is_bounded_by_segment_writes(
    history: Seq<Option<u64>>,
    current: Option<u64>,
    checkpoint: int,
)
    requires 0 <= checkpoint <= history.len(),
    ensures
        (if current != state_at(history, current, checkpoint) { 1int } else { 0int })
            <= history.len() - checkpoint,
{
    if checkpoint == history.len() {
        assert(current == state_at(history, current, checkpoint));
    }
}

}

fn main() {}
