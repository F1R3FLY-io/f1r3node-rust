use std::cell::Cell;

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Metric {
    RootCopies,
    RootBytes,
    BorrowedRegions,
    OriginalBytes,
    ShallowBytes,
    CompactBytes,
    ResidentBytes,
    ArenaCount,
    FinishAttempts,
    FinishSuccesses,
    WorkSteps,
    SharedImports,
    FieldVisits,
    MessageNodes,
    ParCuts,
    RootKeys,
    RootKeyBytes,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    CopyRoot(usize),
    BorrowRegion,
    EmitOriginal(usize),
    EmitShallow(usize),
    EmitCompact(usize),
    DecodeResident(usize),
    CreateArena,
    BeginFinish,
    FinishOk,
    VisitWork,
    ImportShared,
    VisitField,
    PublishNode,
    CutPar,
    EmitRootKey(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals([usize; 17]);

impl Totals {
    pub fn get(&self, metric: Metric) -> usize { self.0[metric as usize] }

    fn increment(&mut self, metric: Metric, amount: usize) {
        let value = &mut self.0[metric as usize];
        *value = value
            .checked_add(amount)
            .expect("substitution cost counter overflow");
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::CopyRoot(n) => {
                self.increment(Metric::RootCopies, 1);
                self.increment(Metric::RootBytes, n);
            }
            Event::BorrowRegion => self.increment(Metric::BorrowedRegions, 1),
            Event::EmitOriginal(n) => self.increment(Metric::OriginalBytes, n),
            Event::EmitShallow(n) => self.increment(Metric::ShallowBytes, n),
            Event::EmitCompact(n) => self.increment(Metric::CompactBytes, n),
            Event::DecodeResident(n) => self.increment(Metric::ResidentBytes, n),
            Event::CreateArena => self.increment(Metric::ArenaCount, 1),
            Event::BeginFinish => self.increment(Metric::FinishAttempts, 1),
            Event::FinishOk => self.increment(Metric::FinishSuccesses, 1),
            Event::VisitWork => self.increment(Metric::WorkSteps, 1),
            Event::ImportShared => self.increment(Metric::SharedImports, 1),
            Event::VisitField => self.increment(Metric::FieldVisits, 1),
            Event::PublishNode => self.increment(Metric::MessageNodes, 1),
            Event::CutPar => self.increment(Metric::ParCuts, 1),
            Event::EmitRootKey(n) => {
                self.increment(Metric::RootKeys, 1);
                self.increment(Metric::RootKeyBytes, n);
            }
        }
    }
}

thread_local! {
    static ACTIVE: Cell<Option<Totals>> = const { Cell::new(None) };
}

pub fn record(event: Event) {
    ACTIVE.with(|slot| {
        if let Some(mut totals) = slot.get() {
            totals.apply(event);
            slot.set(Some(totals));
        }
    });
}

struct CaptureGuard;

impl Drop for CaptureGuard {
    fn drop(&mut self) { ACTIVE.with(|slot| slot.set(None)); }
}

pub fn capture<T>(run: impl FnOnce() -> T) -> (T, Totals) {
    ACTIVE.with(|slot| {
        assert!(slot.get().is_none(), "nested substitution cost capture");
        slot.set(Some(Totals::default()));
    });
    let guard = CaptureGuard;
    let result = run();
    let totals = ACTIVE.with(|slot| slot.get().expect("active substitution cost capture"));
    drop(guard);
    (result, totals)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::rust::rholang::protobuf_cursor::{CanonicalByteRange, ProtobufRegion};

    fn event(tag: u8, n: usize) -> Event {
        match tag % 15 {
            0 => Event::CopyRoot(n),
            1 => Event::BorrowRegion,
            2 => Event::EmitOriginal(n),
            3 => Event::EmitShallow(n),
            4 => Event::EmitCompact(n),
            5 => Event::DecodeResident(n),
            6 => Event::CreateArena,
            7 => Event::BeginFinish,
            8 => Event::FinishOk,
            9 => Event::VisitWork,
            10 => Event::ImportShared,
            11 => Event::VisitField,
            12 => Event::PublishNode,
            13 => Event::CutPar,
            _ => Event::EmitRootKey(n),
        }
    }

    proptest! {
        #[test]
        fn scoped_cost_fold_matches_event_reference(items in prop::collection::vec((0u8..15, 0usize..2048), 0..128), split in 0usize..128) {
            let cut = split.min(items.len());
            let run = |values: &[(u8, usize)]| capture(|| {
                for &(tag, n) in values { record(event(tag, n)); }
            }).1;
            let all = run(&items);
            let left = run(&items[..cut]);
            let right = run(&items[cut..]);
            let mut expected = [0usize; 17];
            for &(tag, n) in &items {
                let pairs: &[(usize, usize)] = match tag {
                    0 => &[(0, 1), (1, n)], 1 => &[(2, 1)], 2 => &[(3, n)],
                    3 => &[(4, n)], 4 => &[(5, n)], 5 => &[(6, n)],
                    6 => &[(7, 1)], 7 => &[(8, 1)], 8 => &[(9, 1)],
                    9 => &[(10, 1)], 10 => &[(11, 1)], 11 => &[(12, 1)],
                    12 => &[(13, 1)], 13 => &[(14, 1)], _ => &[(15, 1), (16, n)],
                };
                for &(index, amount) in pairs { expected[index] += amount; }
            }
            prop_assert_eq!(all.0, expected);
            for i in 0..17 {
                prop_assert_eq!(all.0[i], left.0[i] + right.0[i]);
                prop_assert!(left.0[i] <= all.0[i]);
            }
            let inert: Vec<_> = items.iter().copied().filter(|(tag, _)| !matches!(tag, 6..=8)).collect();
            let inert = run(&inert);
            prop_assert_eq!((inert.0[7], inert.0[8], inert.0[9]), (0, 0, 0));
        }
    }

    #[test]
    fn cost_scopes_are_isolated_panic_safe_and_checked() {
        record(Event::CreateArena);
        assert_eq!(capture(|| ()).1, Totals::default());
        let (_, first) = capture(|| {
            record(Event::VisitWork);
            std::thread::spawn(|| record(Event::VisitWork))
                .join()
                .unwrap();
            assert!(std::panic::catch_unwind(|| capture(|| ())).is_err());
            record(Event::VisitWork);
        });
        assert_eq!(first.get(Metric::WorkSteps), 2);
        assert_eq!(capture(|| ()).1, Totals::default());
        assert!(std::panic::catch_unwind(|| capture(|| {
            record(Event::CopyRoot(usize::MAX));
            record(Event::CopyRoot(1));
        }))
        .is_err());
        assert_eq!(capture(|| ()).1, Totals::default());
        assert!(
            std::panic::catch_unwind(|| capture(|| panic!("intentional capture unwind"))).is_err()
        );
        assert_eq!(capture(|| ()).1, Totals::default());
    }

    #[test]
    fn shared_regions_record_handles_without_body_copies() {
        let root = ProtobufRegion::copy_from_slice(&[1, 2, 3, 4]);
        let (_, totals) = capture(|| {
            let part = root
                .subregion(CanonicalByteRange { start: 1, end: 4 })
                .unwrap();
            let leaf = part
                .subregion(CanonicalByteRange { start: 1, end: 2 })
                .unwrap();
            assert_eq!(leaf.as_bytes().as_ptr(), root.as_bytes()[2..].as_ptr());
            assert_eq!(leaf.absolute_range(), 2..3);
            assert!(root
                .subregion(CanonicalByteRange { start: 0, end: 5 })
                .is_none());
        });
        assert_eq!(totals.get(Metric::BorrowedRegions), 2);
        assert_eq!(totals.get(Metric::SharedImports), 0);
        for metric in [
            Metric::RootBytes,
            Metric::OriginalBytes,
            Metric::ShallowBytes,
            Metric::CompactBytes,
            Metric::ResidentBytes,
        ] {
            assert_eq!(totals.get(metric), 0);
        }
    }

    #[test]
    fn repeated_finish_calls_are_observable() {
        let mut arena = super::super::ParConstruction::new();
        let root = arena.empty_par().unwrap();
        let (_, totals) = capture(|| {
            arena.finish(root).unwrap();
            arena.finish(root).unwrap();
        });
        assert_eq!(totals.get(Metric::ArenaCount), 0);
        assert_eq!(totals.get(Metric::FinishAttempts), 2);
        assert_eq!(totals.get(Metric::FinishSuccesses), 2);
    }

    #[test]
    fn actual_resident_decode_records_input_bytes() {
        use crate::rhoapi::Expr;
        use crate::rust::utils::new_gint_par;
        let input = new_gint_par(123, Vec::new(), false);
        let mut observed = 0;
        input.visit_encoded::<Expr>(|entry| {
            let payload_len = entry.payload().len();
            let (decoded, costs) = capture(|| entry.decode().unwrap());
            assert_eq!(
                decoded.expr_instance,
                Some(crate::rhoapi::expr::ExprInstance::GInt(123))
            );
            assert_eq!(costs.get(Metric::ResidentBytes), payload_len);
            assert!(costs.get(Metric::ResidentBytes) > 0);
            observed += 1;
        });
        assert_eq!(observed, 1);
    }

    #[test]
    fn actual_shared_import_records_no_body_copy() {
        use crate::rhoapi::{Par, Send};
        use crate::rust::rholang::protobuf_encoder;
        use crate::rust::utils::new_gint_par;

        let input = Send {
            data: vec![new_gint_par(7, Vec::new(), false)],
            ..Send::default()
        };
        let shell = Send {
            data: vec![Par::default()],
            ..Send::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&input);
        let region = ProtobufRegion::from(bytes.clone());
        let pointer = region.as_bytes().as_ptr();
        let mut arena = super::super::ParConstruction::new();
        let (component, totals) = capture(|| {
            arena
                .import_admitted_incomplete_component_region(&shell, region)
                .unwrap()
                .unwrap()
        });
        let node = &arena.protobuf_nodes[arena.components[component.0].protobuf];
        let [super::super::ProtoPiece::SharedBytes(shared)] = node.pieces.as_slice() else {
            panic!("expected shared bytes");
        };
        assert_eq!(shared.as_ptr(), pointer);
        assert_eq!(shared.as_ref().as_ref(), bytes);
        assert_eq!(totals.get(Metric::SharedImports), 1);
        for metric in [
            Metric::RootCopies,
            Metric::RootBytes,
            Metric::OriginalBytes,
            Metric::ShallowBytes,
            Metric::CompactBytes,
            Metric::ResidentBytes,
        ] {
            assert_eq!(totals.get(metric), 0, "{totals:?}");
        }
    }

    #[test]
    fn actual_emission_modes_and_planning_counts_match_returned_bytes() {
        use crate::rhoapi::expr::ExprInstance;
        use crate::rhoapi::{EList, Expr, Par};
        use crate::rust::rholang::match_schema_tables::EXPR_RHO_MATCH_PROGRAM;
        use crate::rust::rholang::protobuf_encoder;
        use crate::rust::rholang::protobuf_region::cut_at_par;
        let input = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString("payload".repeat(64))),
                }])],
                ..EList::default()
            })),
        };
        let bytes = protobuf_encoder::encode_to_vec(&input);
        let source = ProtobufRegion::from(bytes.clone());
        let (plan, planning) = capture(|| cut_at_par(source, &EXPR_RHO_MATCH_PROGRAM).unwrap());
        assert_eq!(planning.get(Metric::FieldVisits), plan.stats().field_visits);
        assert_eq!(
            planning.get(Metric::MessageNodes),
            plan.stats().message_nodes
        );
        assert_eq!(planning.get(Metric::ParCuts), plan.cuts().len());
        assert!(planning.get(Metric::ParCuts) > 0);
        let (original, original_cost) = capture(|| plan.reconstructed_bytes());
        let (shallow, shallow_cost) = capture(|| plan.shallow_bytes());
        let (compact, compact_cost) = capture(|| plan.compact_shell_bytes());
        assert_eq!(original.as_ref(), bytes);
        assert!(shallow.len() < original.len());
        assert!(compact.len() < shallow.len());
        for (actual, metric, n) in [
            (original_cost, Metric::OriginalBytes, original.len()),
            (shallow_cost, Metric::ShallowBytes, shallow.len()),
            (compact_cost, Metric::CompactBytes, compact.len()),
        ] {
            let mut expected = Totals::default();
            expected.increment(metric, n);
            assert_eq!(actual, expected);
        }
    }
}
