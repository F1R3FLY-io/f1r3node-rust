use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::utils::new_gstring_par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

use super::*;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
}

struct MeasuredAllocator;

fn measure(bytes: usize) {
    let _ = ALLOCATED.try_with(|total| {
        if let Some(current) = total.get() {
            total.set(Some(current.checked_add(bytes).unwrap()));
        }
    });
}

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            measure(layout.size());
        }
        result
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            measure(size);
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

struct Measurement;

impl Drop for Measurement {
    fn drop(&mut self) { ALLOCATED.with(|total| total.set(None)); }
}

fn clone_fits<T: CloneBacking + Clone>(value: &T) {
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
    reserve(value, &host).unwrap();
    ALLOCATED.with(|total| {
        assert!(total.get().is_none());
        total.set(Some(0));
    });
    let measurement = Measurement;
    let copy = std::hint::black_box(value.clone());
    let actual = ALLOCATED.with(|total| total.get().unwrap());
    drop(measurement);
    assert!(
        actual as u64 <= host.usage(HostWorkDimension::SearchStateBytes).get(),
        "clone requested {actual} bytes, reservation was {}",
        host.usage(HostWorkDimension::SearchStateBytes).get()
    );
    drop(copy);
}

fn term() -> impl Strategy<Value = Par> {
    (prop::collection::vec(any::<u8>(), 0..256), 0_usize..32)
        .prop_map(|(bytes, count)| Par {
            exprs: vec![
                Expr {
                    expr_instance: Some(expr::ExprInstance::GByteArray(bytes))
                };
                count
            ],
            locally_free: vec![7; count],
            ..Default::default()
        })
        .prop_recursive(3, 64, 6, |child| {
            prop::collection::vec(child, 0..6).prop_map(|children| Par {
                sends: vec![Send {
                    data: children.clone(),
                    ..Default::default()
                }],
                news: vec![New {
                    injections: children
                        .into_iter()
                        .enumerate()
                        .map(|(index, child)| (index.to_string(), child))
                        .collect(),
                    ..Default::default()
                }],
                ..Default::default()
            })
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn native_source_allocations_fit_prepaid_backing_and_preserve_typed_identities(par in term(), persistent in any::<bool>()) {
        use rspace_plus_plus::rspace::hashing::native_source;
        use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

        let data = ListParWithRandom { pars: vec![par.clone()], ..Default::default() };
        let channels = vec![par.clone(), Par::default(), par.clone()];
        let patterns = vec![BindPattern { patterns: vec![par.clone()], ..Default::default() }; 3];
        let continuation = TaggedContinuation::default();
        let reserved = Cell::new(0_usize);
        let meter = |_: usize, _: usize, backing: usize| {
            reserved.set(reserved.get().checked_add(backing).unwrap());
            Ok(())
        };
        ALLOCATED.with(|total| { assert!(total.get().is_none()); total.set(Some(0)); });
        let measurement = Measurement;
        let producer = native_source::produce(&par, &data, persistent, &meter).unwrap();
        let consumer = native_source::consume(&channels, &patterns, &continuation, persistent, &meter).unwrap();
        let actual = ALLOCATED.with(|total| total.get().unwrap());
        drop(measurement);
        prop_assert!(actual <= reserved.get(), "requested {}, reserved {}", actual, reserved.get());
        prop_assert_eq!(bincode::serialize(&producer).unwrap(), bincode::serialize(&Produce::create(&par, &data, persistent)).unwrap());
        prop_assert_eq!(consumer, Consume::create(&channels, &patterns, &continuation, persistent));
    }

    #[test]
    fn generated_deep_clones_fit_reserved_backing(par in term()) {
        clone_fits(&par);
        clone_fits(&CostAuthority { regions: vec![CostRegion {
            instance_id: vec![0; 32],
            signature: Some(CostSignature { value: Some(cost_signature::Value::Quote(par)) }),
        }] });
    }

    #[test]
    fn inspection_preserves_copy_traversal_without_reserving_payload_copies(par in term()) {
        let limits = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
        let copy = HostWorkBudget::new(limits.clone());
        let read = HostWorkBudget::new(limits);
        let before = par.clone();
        reserve(&par, &copy).unwrap();
        inspect(&par, &read).unwrap();
        prop_assert_eq!(&par, &before);
        for dimension in [HostWorkDimension::VerificationOperations, HostWorkDimension::VerificationBytes] {
            prop_assert_eq!(read.usage(dimension), copy.usage(dimension));
        }
        prop_assert!(read.usage(HostWorkDimension::SearchStateBytes).get() <= copy.usage(HostWorkDimension::SearchStateBytes).get());
    }

    #[test]
    fn inspection_accepts_exact_budgets_and_rejects_each_smaller_dimension(par in term()) {
        let measured = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
        ALLOCATED.with(|total| { assert!(total.get().is_none()); total.set(Some(0)); });
        let measurement = Measurement;
        inspect(&par, &measured).unwrap();
        let actual = ALLOCATED.with(|total| total.get().unwrap());
        drop(measurement);
        prop_assert!(actual as u64 <= measured.usage(HostWorkDimension::SearchStateBytes).get());
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(0));
        for dimension in HostWorkDimension::ALL {
            exact.set(dimension, HostWorkLimit::new(measured.usage(dimension).get()));
        }
        let before = par.clone();
        let host = HostWorkBudget::new(exact.clone());
        inspect(&par, &host).unwrap();
        for dimension in HostWorkDimension::ALL {
            let required = measured.usage(dimension).get();
            prop_assert_eq!(host.usage(dimension).get(), required);
            if required > 0 {
                let mut short = exact.clone();
                short.set(dimension, HostWorkLimit::new(required - 1));
                let rejected = HostWorkBudget::new(short);
                prop_assert!(matches!(inspect(&par, &rejected), Err(InterpreterError::HostWorkRejected)));
            }
        }
        prop_assert_eq!(par, before);
    }

    #[test]
    fn hash_capacity_not_only_length_controls_clone_backing(capacity in 0_usize..2048, retained in 0_usize..16) {
        let mut map = HashMap::with_capacity(capacity);
        for index in 0..retained {
            map.insert(new_gstring_par(index.to_string(), Vec::new(), false), MergeType::IntegerAdd);
        }
        clone_fits(&map);
        map.clear();
        clone_fits(&map);
    }
}

#[test]
fn source_inspection_covers_guards_authorities_stacks_and_random_state() {
    let limits = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
    let payload = Par {
        exprs: vec![Expr {
            expr_instance: Some(expr::ExprInstance::GByteArray(vec![1; 4096])),
        }],
        ..Default::default()
    };
    let authority = CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![2; 32],
            signature: Some(CostSignature {
                value: Some(cost_signature::Value::Quote(payload.clone())),
            }),
        }],
    };
    let data = ListParWithRandom {
        pars: vec![payload.clone()],
        random_state: vec![3; 256],
        cost_authority: Some(authority.clone()),
        cost_stack: Some(CostStack {
            cells: vec![authority.regions[0].signature.clone().unwrap()],
        }),
    };
    let continuation = TaggedContinuation {
        tagged_cont: Some(tagged_continuation::TaggedCont::ParBody(ParWithRandom {
            body: Some(payload.clone()),
            random_state: vec![4; 256],
        })),
        guard: Some(payload),
        cost_authority: Some(authority),
    };
    for empty in [false, true] {
        let host = HostWorkBudget::new(if empty {
            HostWorkLimits::uniform(HostWorkLimit::new(0))
        } else {
            limits.clone()
        });
        assert_eq!(inspect(&data, &host).is_ok(), !empty);
        assert_eq!(inspect(&continuation, &host).is_ok(), !empty);
        if !empty {
            assert!(host.usage(HostWorkDimension::VerificationBytes).get() > 4096 * 4);
            assert!(host.usage(HostWorkDimension::SearchStateBytes).get() < 4096);
        }
    }
}

#[test]
fn every_expression_and_container_payload_has_clone_backing() {
    use expr::ExprInstance::*;
    let p = new_gstring_par("payload".repeat(64), vec![9; 128], false);
    let mut expressions = vec![
        GBool(true),
        GInt(1),
        GDouble(1),
        GString("text".repeat(32)),
        GUri("uri".repeat(32)),
        GByteArray(vec![0; 128]),
        GBigInt(vec![0; 128]),
        GBigRat(GBigRational {
            numerator: vec![1; 64],
            denominator: vec![2; 64],
        }),
        GFixedPoint(models::rhoapi::GFixedPoint {
            unscaled: vec![3; 64],
            scale: 5,
        }),
        ENotBody(ENot { p: Some(p.clone()) }),
        ENegBody(ENeg { p: Some(p.clone()) }),
        EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(var::VarInstance::Wildcard(var::WildcardMsg {})),
            }),
        }),
        EListBody(EList {
            ps: vec![p.clone()],
            locally_free: vec![1; 100],
            ..Default::default()
        }),
        ETupleBody(ETuple {
            ps: vec![p.clone()],
            ..Default::default()
        }),
        ESetBody(ESet {
            ps: vec![p.clone()],
            ..Default::default()
        }),
        EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(p.clone()),
                value: Some(p.clone()),
            }],
            ..Default::default()
        }),
        EPathmapBody(EPathMap {
            ps: vec![p.clone()],
            ..Default::default()
        }),
        EZipperBody(EZipper {
            pathmap: Some(EPathMap {
                ps: vec![p.clone()],
                ..Default::default()
            }),
            current_path: vec![vec![2; 128]],
            ..Default::default()
        }),
        EMethodBody(EMethod {
            method_name: "method".repeat(32),
            target: Some(p.clone()),
            arguments: vec![p.clone()],
            ..Default::default()
        }),
        EMatchesBody(EMatches {
            target: Some(p.clone()),
            pattern: Some(p.clone()),
        }),
    ];
    macro_rules! binary {
        ($($variant:ident($ty:ident)),+ $(,)?) => { $(expressions.push($variant($ty { p1: Some(p.clone()), p2: Some(p.clone()) }));)+ };
    }
    binary!(
        EMultBody(EMult),
        EDivBody(EDiv),
        EPlusBody(EPlus),
        EMinusBody(EMinus),
        ELtBody(ELt),
        ELteBody(ELte),
        EGtBody(EGt),
        EGteBody(EGte),
        EEqBody(EEq),
        ENeqBody(ENeq),
        EAndBody(EAnd),
        EOrBody(EOr),
        EPercentPercentBody(EPercentPercent),
        EPlusPlusBody(EPlusPlus),
        EMinusMinusBody(EMinusMinus),
        EModBody(EMod)
    );
    let compound = CostSignature {
        value: Some(cost_signature::Value::Compound(CostSignatureCompound {
            elements: vec![
                CostSignature {
                    value: Some(cost_signature::Value::Quote(p.clone())),
                },
                CostSignature {
                    value: Some(cost_signature::Value::Name(p.clone())),
                },
                CostSignature {
                    value: Some(cost_signature::Value::Ground(vec![0; 128])),
                },
            ],
        })),
    };
    let value = Par {
        exprs: expressions
            .into_iter()
            .map(|expr_instance| Expr {
                expr_instance: Some(expr_instance),
            })
            .collect(),
        sends: vec![Send {
            chan: Some(p.clone()),
            data: vec![p.clone()],
            ..Default::default()
        }],
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![p.clone()],
                source: Some(p.clone()),
                cost_signature: Some(compound.clone()),
                ..Default::default()
            }],
            body: Some(p.clone()),
            condition: Some(p.clone()),
            ..Default::default()
        }],
        news: vec![New {
            p: Some(p.clone()),
            uri: vec!["uri".repeat(100)],
            injections: BTreeMap::from([("uri".repeat(64), p.clone())]),
            ..Default::default()
        }],
        matches: vec![Match {
            target: Some(p.clone()),
            cases: vec![MatchCase {
                pattern: Some(p.clone()),
                source: Some(p.clone()),
                guard: Some(p.clone()),
                ..Default::default()
            }],
            ..Default::default()
        }],
        conditionals: vec![If {
            condition: Some(p.clone()),
            if_true: Some(p.clone()),
            if_false: Some(p.clone()),
            ..Default::default()
        }],
        bundles: vec![Bundle {
            body: Some(p.clone()),
            ..Default::default()
        }],
        connectives: vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnAndBody(
                ConnectiveBody {
                    ps: vec![p.clone()],
                },
            )),
        }],
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(g_unforgeable::UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: 1,
                public_key: vec![0; 128],
            })),
        }],
        cost_signed_terms: vec![CostSignedTerm {
            body: Some(p),
            signature: Some(compound.clone()),
        }],
        cost_stacks: vec![CostStack {
            cells: vec![compound],
        }],
        locally_free: vec![0; 256],
        ..Default::default()
    };
    clone_fits(&value);
}

#[test]
fn exhausted_walk_preserves_source_and_does_not_clone_payload() {
    let source = new_gstring_par("payload".repeat(1000), vec![1; 512], false);
    let expected = source.clone();
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000));
    limits.set(HostWorkDimension::SearchStateBytes, HostWorkLimit::new(0));
    let host = HostWorkBudget::new(limits);
    assert!(matches!(
        reserve(&source, &host),
        Err(InterpreterError::HostWorkRejected)
    ));
    assert!(host.is_rejected());
    assert_eq!(host.usage(HostWorkDimension::SearchStateBytes).get(), 0);
    assert_eq!(source, expected);
}
