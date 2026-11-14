use std::collections::VecDeque;
use std::num::NonZeroUsize;

use pathmap::utils::{BitMask, ByteMask};

use super::{CandidateId, RetainedSetAutomaton, Terminal, TerminalId, mark, mark_without_residual};
use crate::rspace::bag::{Bag, BagReadCursor};
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::rho_path::{RhoPathQuery, RhoPathV1};

type Fact = (Vec<u8>, TerminalId);

#[derive(Debug, Clone, Copy)]
struct FlatState {
    terminal: Option<TerminalId>,
    label_end: usize,
    first_child: usize,
    mask: Option<NonZeroUsize>,
}

impl FlatState {
    const EMPTY: Self = Self {
        terminal: None,
        label_end: 0,
        first_child: 0,
        mask: None,
    };
}

#[derive(Debug)]
struct FlatTable {
    states: Vec<FlatState>,
    masks: Vec<ByteMask>,
    labels: Vec<u8>,
}

#[derive(Debug)]
pub struct FlatDispatch {
    tables: Vec<FlatTable>,
}

pub(super) struct FlatFacts {
    positions: Vec<Vec<Fact>>,
}

pub fn flat_tables(automaton: &RetainedSetAutomaton) -> Option<&FlatDispatch> {
    automaton.flat.as_ref()
}

impl FlatFacts {
    pub(super) fn for_candidates<P: Clone, K: Clone>(
        candidates: &[WaitingContinuation<P, K>],
        bind_position_count: usize,
    ) -> Self {
        let mut pattern_counts = vec![0usize; bind_position_count];
        for candidate in candidates {
            for count in &mut pattern_counts[..candidate.patterns.len()] {
                *count += 1;
            }
        }
        Self {
            positions: pattern_counts.into_iter().map(Vec::with_capacity).collect(),
        }
    }

    pub(super) fn push(
        &mut self,
        position: usize,
        path: Vec<u8>,
        terminals: &mut Vec<Terminal>,
        candidate: CandidateId,
        query: Option<RhoPathQuery>,
    ) {
        let terminal = TerminalId::from_index(terminals.len());
        terminals.push(Terminal {
            candidate,
            previous: None,
            query,
        });
        self.positions[position].push((path, terminal));
    }

    pub(super) fn build(self, terminals: &mut [Terminal]) -> FlatDispatch {
        FlatDispatch {
            tables: self
                .positions
                .into_iter()
                .map(|facts| FlatTable::build(facts, terminals))
                .collect(),
        }
    }
}

impl FlatDispatch {
    pub(super) fn terminal_state_count(&self) -> usize {
        self.tables
            .iter()
            .map(|table| {
                table
                    .states
                    .iter()
                    .filter(|state| state.terminal.is_some())
                    .count()
            })
            .sum()
    }

    pub(super) fn matching_candidates_with_resting(
        &self,
        automaton: &RetainedSetAutomaton,
        datum_path: &[u8],
        bind_positions: &[usize],
        resting: Option<&Bag>,
    ) -> Vec<bool> {
        let mut selected = vec![false; automaton.candidate_count];
        for position in bind_positions {
            let Some(table) = self.tables.get(*position) else {
                continue;
            };
            table.mark_fresh(&automaton.terminals, datum_path, &mut selected);
            if let Some(resting) = resting {
                table.mark_resting(
                    &automaton.terminal_depths[*position],
                    &automaton.terminals,
                    resting,
                    &mut selected,
                );
            }
        }
        selected
    }
}

impl FlatTable {
    fn build(mut facts: Vec<Fact>, terminals: &mut [Terminal]) -> Self {
        facts.sort_by(|left, right| left.0.cmp(&right.0));
        keep_newest_terminal_per_path(&mut facts, terminals);
        Self::lay_out(&facts)
    }

    fn lay_out(keys: &[Fact]) -> Self {
        let state_bound = 2 * keys.len().max(1) - 1;
        let mut table = Self {
            states: Vec::with_capacity(state_bound),
            masks: Vec::with_capacity(state_bound - 1),
            labels: Vec::with_capacity(keys.iter().map(|(key, _)| key.len()).sum()),
        };
        let mut pending = VecDeque::with_capacity(keys.len());
        table.states.push(FlatState::EMPTY);
        pending.push_back((0, 0, keys.len(), 0));
        while let Some((state, mut low, high, mut depth)) = pending.pop_front() {
            if let (Some((first_key, _)), Some((last_key, _))) =
                (keys[low..high].first(), keys[low..high].last())
            {
                let shared = first_key[depth..]
                    .iter()
                    .zip(&last_key[depth..])
                    .take_while(|(first, last)| first == last)
                    .count();
                table
                    .labels
                    .extend_from_slice(&first_key[depth..depth + shared]);
                depth += shared;
            }
            table.states[state].label_end = table.labels.len();
            if let Some((_, terminal)) = keys[low..high]
                .first()
                .filter(|(key, _)| key.len() == depth)
            {
                table.states[state].terminal = Some(*terminal);
                low += 1;
            }
            if low == high {
                continue;
            }
            let first_child = table.states.len();
            let mut children = ByteMask::EMPTY;
            while low < high {
                let byte = keys[low].0[depth];
                let end = low + keys[low..high].partition_point(|(key, _)| key[depth] <= byte);
                children.set_bit(byte);
                pending.push_back((table.states.len(), low, end, depth + 1));
                table.states.push(FlatState::EMPTY);
                low = end;
            }
            table.masks.push(children);
            table.states[state].first_child = first_child;
            table.states[state].mask = NonZeroUsize::new(table.masks.len());
        }
        table.states.shrink_to_fit();
        table.masks.shrink_to_fit();
        table.labels.shrink_to_fit();
        table
    }

    fn label(&self, state: usize) -> &[u8] {
        let start = match state.checked_sub(1) {
            Some(previous) => self.states[previous].label_end,
            None => 0,
        };
        &self.labels[start..self.states[state].label_end]
    }

    fn children(&self, state: &FlatState) -> ByteMask {
        state
            .mask
            .map_or(ByteMask::EMPTY, |mask| self.masks[mask.get() - 1])
    }

    fn child(&self, state: &FlatState, byte: u8) -> Option<usize> {
        let mask = &self.masks[state.mask?.get() - 1];
        mask.test_bit(byte)
            .then(|| state.first_child + usize::from(mask.index_of(byte)))
    }

    fn terminal_at(&self, path: &[u8]) -> Option<TerminalId> {
        let mut state = 0;
        let mut rest = path;
        loop {
            rest = rest.strip_prefix(self.label(state))?;
            let entry = &self.states[state];
            let Some((byte, tail)) = rest.split_first() else {
                return entry.terminal;
            };
            state = self.child(entry, *byte)?;
            rest = tail;
        }
    }

    fn mark_fresh(&self, terminals: &[Terminal], raw_datum_path: &[u8], selected: &mut [bool]) {
        let mut bytes = RhoPathV1::relative_datum_structure_prefix_bytes(raw_datum_path);
        let mut state = 0;
        loop {
            for expected in self.label(state) {
                if bytes.next() != Some(*expected) {
                    return;
                }
            }
            let entry = &self.states[state];
            mark(entry.terminal, terminals, raw_datum_path, selected);
            let Some(child) = bytes.next().and_then(|byte| self.child(entry, byte)) else {
                return;
            };
            state = child;
        }
    }

    fn mark_resting(
        &self,
        terminal_depths: &[usize],
        terminals: &[Terminal],
        resting: &Bag,
        selected: &mut [bool],
    ) {
        if resting.is_empty() {
            return;
        }

        resting.with_read_cursor(|data| {
            self.mark_resting_with_cursor(terminal_depths, terminals, resting, data, selected);
        });
    }

    fn mark_resting_with_cursor(
        &self,
        terminal_depths: &[usize],
        terminals: &[Terminal],
        resting: &Bag,
        mut data: BagReadCursor<'_, 'static>,
        selected: &mut [bool],
    ) {
        let root = &self.states[0];
        let root_label = self.label(0);
        if root_label.is_empty() {
            mark_without_residual(root.terminal, terminals, selected);
        }
        if resting.distinct_len() == 1 {
            let has_value = data.to_next_val();
            assert!(has_value, "a nonempty singleton Bag has one value");
            let path = data.path();
            let first_non_root = terminal_depths.partition_point(|depth| *depth == 0);
            let end = terminal_depths.partition_point(|depth| *depth <= path.len());
            match terminal_depths.get(first_non_root..end).unwrap_or_default() {
                [] => return,
                [depth] => {
                    mark_without_residual(self.terminal_at(&path[..*depth]), terminals, selected);
                    return;
                }
                _ => data.reset(),
            }
        }

        if data.descend_to_existing(root_label) < root_label.len() {
            return;
        }
        if !root_label.is_empty() {
            mark_without_residual(root.terminal, terminals, selected);
        }
        let mut frames =
            vec![(0, root_label.len(), (self.children(root) & data.child_mask()).iter())];
        while let Some(frame) = frames.last_mut() {
            let parent = &self.states[frame.0];
            let Some(byte) = frame.2.next() else {
                let descended = frame.1;
                frames.pop();
                if !frames.is_empty() {
                    ascend(&mut data, descended);
                }
                continue;
            };
            let child = parent.first_child + usize::from(self.children(parent).index_of(byte));
            data.descend_to_byte(byte);
            let label = self.label(child);
            let descended = data.descend_to_existing(label);
            if descended < label.len() {
                ascend(&mut data, descended + 1);
                continue;
            }
            let entry = &self.states[child];
            mark_without_residual(entry.terminal, terminals, selected);
            frames.push((child, descended + 1, (self.children(entry) & data.child_mask()).iter()));
        }
    }
}

fn keep_newest_terminal_per_path(facts: &mut Vec<Fact>, terminals: &mut [Terminal]) {
    let mut key_count = 0usize;
    for index in 0..facts.len() {
        match key_count.checked_sub(1) {
            Some(last) if facts[last].0 == facts[index].0 => {
                let newest = facts[index].1;
                terminals
                    .get_mut(newest.index())
                    .expect("a compiled terminal identifier always addresses its arena")
                    .previous = Some(facts[last].1);
                facts[last].1 = newest;
            }
            _ => {
                facts.swap(key_count, index);
                key_count += 1;
            }
        }
    }
    facts.truncate(key_count);
}

fn ascend(data: &mut BagReadCursor<'_, '_>, bytes: usize) {
    for _ in 0..bytes {
        let ascended = data.ascend_byte();
        debug_assert!(ascended, "the cursor climbs back only the bytes it descended");
    }
}
