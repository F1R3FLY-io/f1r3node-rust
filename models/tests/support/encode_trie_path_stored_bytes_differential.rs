use proptest::prelude::*;
use prost::encoding::{encode_key, encode_varint, WireType};

use super::*;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::{Connective, ENot, ESet, EVar, GDeployId, Var};

#[allow(clippy::duplicate_mod)]
#[path = "../par_corpus/mod.rs"]
mod par_corpus;

const SHORT_STRINGS: [&str; 16] = [
    "ack", "add", "balance", "deposit", "get", "init", "lookup", "nonce", "owner", "purse", "read",
    "result", "set", "status", "transfer", "value",
];

enum FrozenContext {
    Region(Vec<Vec<u8>>),
}

enum FrozenPar<'p> {
    Borrowed(&'p Par),
    Owned(Par),
}

impl FrozenPar<'_> {
    fn as_par(&self) -> &Par {
        match self {
            Self::Borrowed(par) => par,
            Self::Owned(par) => par,
        }
    }
}

enum FrozenOp<'p> {
    BorrowedStableExpr(&'p Expr),
    Segment {
        par: FrozenPar<'p>,
        known_stable: Option<bool>,
        top_level: bool,
    },
    CloseNestedRegion,
    EmitTerminator,
}

struct FrozenMachine<'p> {
    root: Vec<u8>,
    detours: Vec<FrozenContext>,
    ops: Vec<FrozenOp<'p>>,
    top_level_stable: Option<bool>,
}

impl<'p> FrozenMachine<'p> {
    fn new() -> Self {
        FrozenMachine {
            root: Vec::new(),
            detours: Vec::new(),
            ops: Vec::new(),
            top_level_stable: None,
        }
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        match self.detours.last_mut() {
            Some(FrozenContext::Region(_)) => {
                unreachable!("frozen encoder invariant: raw emit into a region context")
            }
            None => {
                self.root.extend_from_slice(bytes);
                Ok(())
            }
        }
    }

    fn emit_byte(&mut self, byte: u8) -> Result<(), CodecError> { self.emit(&[byte]) }

    fn emit_uv(&mut self, value: u64) -> Result<(), CodecError> {
        let mut scratch = Vec::with_capacity(10);
        write_uv(&mut scratch, value);
        self.emit(&scratch)
    }

    fn emit_unary_elist_segment(&mut self, depth: usize, terminal: i64) -> Result<(), CodecError> {
        for _ in 0..depth {
            self.emit_byte(tag::ELIST)?;
            self.emit_uv(1)?;
        }
        self.emit_byte(tag::GINT)?;
        self.emit_uv(zigzag64(terminal))
    }

    fn emit_unary_etuple_segment(&mut self, depth: usize, terminal: i64) -> Result<(), CodecError> {
        for _ in 0..depth {
            self.emit_byte(tag::ETUPLE)?;
            self.emit_uv(1)?;
        }
        self.emit_byte(tag::GINT)?;
        self.emit_uv(zigzag64(terminal))
    }

    fn push_path_ops(
        &mut self,
        par: FrozenPar<'p>,
        known_stable: Option<bool>,
    ) -> Result<(), CodecError> {
        let par_ref = par.as_par();
        let stable = match known_stable {
            Some(stable) => stable,
            None => eval_stable_par(par_ref),
        };
        if self.top_level_stable.is_none() {
            self.top_level_stable = Some(stable);
        }
        if stable {
            if let Some((depth, terminal)) = encoded_unary_elist_parts(par_ref) {
                if depth > 0 {
                    self.emit_unary_elist_segment(depth - 1, terminal)?;
                    self.emit_byte(tag::TERM)?;
                    return Ok(());
                }
            }
        }
        if let Some(list) = split_carrier_list(par_ref) {
            self.push_split_list(list, stable);
        } else {
            self.ops.push(FrozenOp::Segment {
                par,
                known_stable: Some(stable),
                top_level: true,
            });
        }
        Ok(())
    }

    fn push_split_list(&mut self, list: EList, stable: bool) {
        self.ops.push(FrozenOp::EmitTerminator);
        for element in list.ps.into_iter().rev() {
            self.ops.push(FrozenOp::Segment {
                par: FrozenPar::Owned(element),
                known_stable: stable.then_some(true),
                top_level: true,
            });
        }
    }

    fn run(&mut self) -> Result<(), CodecError> {
        while let Some(op) = self.ops.pop() {
            match op {
                FrozenOp::BorrowedStableExpr(expr) => self.encode_borrowed_stable_expr(expr)?,
                FrozenOp::Segment {
                    par,
                    known_stable,
                    top_level,
                } => self.encode_segment(par, known_stable, top_level)?,
                FrozenOp::CloseNestedRegion => {
                    let Some(FrozenContext::Region(paths)) = self.detours.pop() else {
                        unreachable!("frozen encoder invariant: CloseNestedRegion without a region")
                    };
                    use pathmap::zipper::{ZipperIteration, ZipperMoving};
                    let region_trie: pathmap::PathMap<()> =
                        paths.iter().map(|path| path.as_slice()).collect();
                    let mut region = Vec::new();
                    let mut zipper = region_trie.read_zipper();
                    while zipper.to_next_val() {
                        let path = zipper.path();
                        write_uv(&mut region, path.len() as u64);
                        region.extend_from_slice(path);
                    }
                    self.emit_uv(region.len() as u64)?;
                    self.emit(&region)?;
                }
                FrozenOp::EmitTerminator => self.emit_byte(tag::TERM)?,
            }
        }
        Ok(())
    }

    fn encode_segment(
        &mut self,
        par: FrozenPar<'p>,
        known_stable: Option<bool>,
        top_level: bool,
    ) -> Result<(), CodecError> {
        let par_ref = par.as_par();
        let stable = match known_stable {
            Some(stable) => stable,
            None => eval_stable_par(par_ref),
        };
        if !stable {
            debug_assert!(
                top_level,
                "frozen escape below a top-level segment position"
            );
            let protobuf_bytes = crate::rust::rholang::protobuf_encoder::encode_to_vec(par_ref);
            self.emit_byte(tag::ESCAPE)?;
            self.emit_uv(protobuf_bytes.len() as u64)?;
            return self.emit(&protobuf_bytes);
        }
        if let Some((depth, terminal)) = encoded_unary_elist_parts(par_ref) {
            return self.emit_unary_elist_segment(depth, terminal);
        }
        if let Some((depth, terminal)) = encoded_unary_etuple_parts(par_ref) {
            return self.emit_unary_etuple_segment(depth, terminal);
        }
        match (
            par_ref.family_cardinality::<Expr>(),
            par_ref.family_cardinality::<GUnforgeable>(),
        ) {
            (0, 1) => {
                let (unforgeable, multiplicity) = par_ref
                    .first::<GUnforgeable>()
                    .expect("resident GUnforgeable key decodes")
                    .expect("stable GPrivate carrier has one unforgeable");
                debug_assert_eq!(multiplicity.get(), 1);
                let Some(UnfInstance::GPrivateBody(private)) = unforgeable.unf_instance else {
                    unreachable!("eval_stable unforgeable leaf must be GPrivate")
                };
                self.emit_byte(tag::GPRIVATE)?;
                self.emit_uv(private.id.len() as u64)?;
                self.emit(&private.id)
            }
            (1, 0) => {
                let (expr, multiplicity) = par_ref
                    .first::<Expr>()
                    .expect("resident Expr key decodes")
                    .expect("stable Expr carrier has one expression");
                debug_assert_eq!(multiplicity.get(), 1);
                let Some(instance) = expr.expr_instance else {
                    unreachable!("eval_stable expr carrier must hold an instance")
                };
                match instance {
                    ExprInstance::GBool(false) => self.emit_byte(tag::GBOOL_FALSE),
                    ExprInstance::GBool(true) => self.emit_byte(tag::GBOOL_TRUE),
                    ExprInstance::GInt(value) => {
                        self.emit_byte(tag::GINT)?;
                        self.emit_uv(zigzag64(value))
                    }
                    ExprInstance::GString(text) => {
                        self.emit_byte(tag::GSTRING)?;
                        self.emit_uv(text.len() as u64)?;
                        self.emit(text.as_bytes())
                    }
                    ExprInstance::GUri(uri) => {
                        self.emit_byte(tag::GURI)?;
                        self.emit_uv(uri.len() as u64)?;
                        self.emit(uri.as_bytes())
                    }
                    ExprInstance::GByteArray(bytes) => {
                        self.emit_byte(tag::GBYTE_ARRAY)?;
                        self.emit_uv(bytes.len() as u64)?;
                        self.emit(&bytes)
                    }
                    ExprInstance::GDouble(bits) => {
                        self.emit_byte(tag::GDOUBLE)?;
                        self.emit(&bits.to_be_bytes())
                    }
                    ExprInstance::GBigInt(bytes) => {
                        self.emit_byte(tag::GBIG_INT)?;
                        self.emit_uv(bytes.len() as u64)?;
                        self.emit(&bytes)
                    }
                    ExprInstance::GBigRat(rational) => {
                        self.emit_byte(tag::GBIG_RAT)?;
                        self.emit_uv(rational.numerator.len() as u64)?;
                        self.emit(&rational.numerator)?;
                        self.emit_uv(rational.denominator.len() as u64)?;
                        self.emit(&rational.denominator)
                    }
                    ExprInstance::GFixedPoint(fixed) => {
                        self.emit_byte(tag::GFIXED_POINT)?;
                        self.emit_uv(fixed.unscaled.len() as u64)?;
                        self.emit(&fixed.unscaled)?;
                        self.emit_uv(u64::from(fixed.scale))
                    }
                    ExprInstance::EListBody(list) => {
                        self.emit_byte(tag::ELIST)?;
                        self.emit_uv(list.ps.len() as u64)?;
                        for child in list.ps.into_iter().rev() {
                            self.ops.push(FrozenOp::Segment {
                                par: FrozenPar::Owned(child),
                                known_stable: Some(true),
                                top_level: false,
                            });
                        }
                        Ok(())
                    }
                    ExprInstance::ETupleBody(tuple) => {
                        self.emit_byte(tag::ETUPLE)?;
                        self.emit_uv(tuple.ps.len() as u64)?;
                        for child in tuple.ps.into_iter().rev() {
                            self.ops.push(FrozenOp::Segment {
                                par: FrozenPar::Owned(child),
                                known_stable: Some(true),
                                top_level: false,
                            });
                        }
                        Ok(())
                    }
                    ExprInstance::EPathmapBody(map) => {
                        if map.mode() == EPathMapMode::Map {
                            self.emit_byte(tag::EPATHMAP_MAP)?;
                            let snapshot = map.trie_snapshot();
                            self.emit_uv(snapshot.len() as u64)?;
                            return self.emit(snapshot);
                        }
                        self.emit_byte(tag::EPATHMAP)?;
                        self.detours.push(FrozenContext::Region(Vec::new()));
                        {
                            use pathmap::zipper::{ZipperIteration, ZipperMoving};
                            let Some(FrozenContext::Region(paths)) = self.detours.last_mut() else {
                                unreachable!("frozen encoder invariant: region pushed above")
                            };
                            let mut zipper = map.entry_trie().set_trie().read_zipper();
                            while zipper.to_next_val() {
                                paths.push(zipper.path().to_vec());
                            }
                        }
                        self.ops.push(FrozenOp::CloseNestedRegion);
                        Ok(())
                    }
                    _ => unreachable!("eval_stable expr outside the stable alphabet"),
                }
            }
            _ => unreachable!("eval_stable par outside the two carrier shapes"),
        }
    }

    fn encode_borrowed_stable_expr(&mut self, expr: &'p Expr) -> Result<(), CodecError> {
        let Some(instance) = expr.expr_instance.as_ref() else {
            unreachable!("stable Expr has a constructor")
        };
        match instance {
            ExprInstance::GBool(false) => self.emit_byte(tag::GBOOL_FALSE),
            ExprInstance::GBool(true) => self.emit_byte(tag::GBOOL_TRUE),
            ExprInstance::GInt(value) => {
                self.emit_byte(tag::GINT)?;
                self.emit_uv(zigzag64(*value))
            }
            ExprInstance::GString(text) => {
                self.emit_byte(tag::GSTRING)?;
                self.emit_uv(text.len() as u64)?;
                self.emit(text.as_bytes())
            }
            ExprInstance::GUri(uri) => {
                self.emit_byte(tag::GURI)?;
                self.emit_uv(uri.len() as u64)?;
                self.emit(uri.as_bytes())
            }
            ExprInstance::GByteArray(bytes) => {
                self.emit_byte(tag::GBYTE_ARRAY)?;
                self.emit_uv(bytes.len() as u64)?;
                self.emit(bytes)
            }
            ExprInstance::GDouble(bits) => {
                self.emit_byte(tag::GDOUBLE)?;
                self.emit(&bits.to_be_bytes())
            }
            ExprInstance::GBigInt(bytes) => {
                self.emit_byte(tag::GBIG_INT)?;
                self.emit_uv(bytes.len() as u64)?;
                self.emit(bytes)
            }
            ExprInstance::GBigRat(rational) => {
                self.emit_byte(tag::GBIG_RAT)?;
                self.emit_uv(rational.numerator.len() as u64)?;
                self.emit(&rational.numerator)?;
                self.emit_uv(rational.denominator.len() as u64)?;
                self.emit(&rational.denominator)
            }
            ExprInstance::GFixedPoint(fixed) => {
                self.emit_byte(tag::GFIXED_POINT)?;
                self.emit_uv(fixed.unscaled.len() as u64)?;
                self.emit(&fixed.unscaled)?;
                self.emit_uv(u64::from(fixed.scale))
            }
            ExprInstance::EListBody(list) => {
                self.emit_byte(tag::ELIST)?;
                self.emit_uv(list.ps.len() as u64)?;
                for child in list.ps.iter().rev() {
                    self.ops.push(FrozenOp::Segment {
                        par: FrozenPar::Borrowed(child),
                        known_stable: Some(true),
                        top_level: false,
                    });
                }
                Ok(())
            }
            ExprInstance::ETupleBody(tuple) => {
                self.emit_byte(tag::ETUPLE)?;
                self.emit_uv(tuple.ps.len() as u64)?;
                for child in tuple.ps.iter().rev() {
                    self.ops.push(FrozenOp::Segment {
                        par: FrozenPar::Borrowed(child),
                        known_stable: Some(true),
                        top_level: false,
                    });
                }
                Ok(())
            }
            ExprInstance::EPathmapBody(map) => {
                if map.mode() == EPathMapMode::Map {
                    self.emit_byte(tag::EPATHMAP_MAP)?;
                    let snapshot = map.trie_snapshot();
                    self.emit_uv(snapshot.len() as u64)?;
                    return self.emit(snapshot);
                }
                self.emit_byte(tag::EPATHMAP)?;
                let mut region = Vec::new();
                {
                    use pathmap::zipper::{ZipperIteration, ZipperMoving};
                    let mut zipper = map.entry_trie().set_trie().read_zipper();
                    while zipper.to_next_val() {
                        let path = zipper.path();
                        write_uv(&mut region, path.len() as u64);
                        region.extend_from_slice(path);
                    }
                }
                self.emit_uv(region.len() as u64)?;
                self.emit(&region)
            }
            _ => unreachable!("stable Expr has a ground constructor"),
        }
    }
}

fn frozen_trie_path_with_stability(par: &Par) -> (Vec<u8>, bool) {
    let mut machine = FrozenMachine::new();
    machine
        .push_path_ops(FrozenPar::Borrowed(par), None)
        .expect("frozen trie path scheduling is total");
    machine.run().expect("frozen trie path encoding is total");
    let stable = machine
        .top_level_stable
        .expect("frozen push_path_ops records the top-level verdict");
    (machine.root, stable)
}

fn frozen_trie_segment(par: &Par) -> Vec<u8> {
    let mut machine = FrozenMachine::new();
    machine.ops.push(FrozenOp::Segment {
        par: FrozenPar::Borrowed(par),
        known_stable: None,
        top_level: true,
    });
    machine
        .run()
        .expect("frozen trie segment encoding is total");
    machine.root
}

fn frozen_stable_expr_segment(expr: &Expr) -> Vec<u8> {
    let mut machine = FrozenMachine::new();
    machine.ops.push(FrozenOp::BorrowedStableExpr(expr));
    machine
        .run()
        .expect("frozen stable Expr segment encoding is total");
    machine.root
}

fn frozen_stable_expr_split_path(expr: &Expr) -> Option<Vec<u8>> {
    let Some(ExprInstance::EListBody(list)) = expr.expr_instance.as_ref() else {
        return None;
    };
    let mut machine = FrozenMachine::new();
    machine.ops.push(FrozenOp::EmitTerminator);
    for child in list.ps.iter().rev() {
        machine.ops.push(FrozenOp::Segment {
            par: FrozenPar::Borrowed(child),
            known_stable: Some(true),
            top_level: true,
        });
    }
    machine
        .run()
        .expect("frozen stable list path encoding is total");
    Some(machine.root)
}

fn assert_matches_frozen_encoder(label: &str, par: &Par) {
    assert_eq!(
        encode_trie_path_with_stability(par),
        frozen_trie_path_with_stability(par),
        "trie path and stability verdict of {label}"
    );
    assert_eq!(
        encode_trie_segment(par),
        frozen_trie_segment(par),
        "trie segment of {label}"
    );
    if eval_stable_par(par) && par.family_cardinality::<Expr>() == 1 {
        let (expr, _) = par
            .first::<Expr>()
            .expect("resident Expr key decodes")
            .expect("stable Expr carrier holds one expression");
        assert_eq!(
            encode_stable_expr_segment(&expr),
            frozen_stable_expr_segment(&expr),
            "stable Expr segment of {label}"
        );
        assert_eq!(
            encode_stable_expr_split_path(&expr),
            frozen_stable_expr_split_path(&expr),
            "stable Expr split path of {label}"
        );
    }
}

fn labelled(label: impl Into<String>, par: Par) -> (String, Par) { (label.into(), par) }

fn gbool(value: bool) -> Par { expr_carrier(ExprInstance::GBool(value)) }

fn gint(value: i64) -> Par { expr_carrier(ExprInstance::GInt(value)) }

fn gstring(text: &str) -> Par { expr_carrier(ExprInstance::GString(text.to_string())) }

fn guri(text: &str) -> Par { expr_carrier(ExprInstance::GUri(text.to_string())) }

fn gbytes(bytes: &[u8]) -> Par { expr_carrier(ExprInstance::GByteArray(bytes.to_vec())) }

fn gdouble_bits(bits: u64) -> Par { expr_carrier(ExprInstance::GDouble(bits)) }

fn gbig_int(bytes: &[u8]) -> Par { expr_carrier(ExprInstance::GBigInt(bytes.to_vec())) }

fn gbig_rat(numerator: &[u8], denominator: &[u8]) -> Par {
    expr_carrier(ExprInstance::GBigRat(GBigRational {
        numerator: numerator.to_vec(),
        denominator: denominator.to_vec(),
    }))
}

fn gfixed_point(unscaled: &[u8], scale: u32) -> Par {
    expr_carrier(ExprInstance::GFixedPoint(GFixedPoint {
        unscaled: unscaled.to_vec(),
        scale,
    }))
}

fn gprivate(id: &[u8]) -> Par { gprivate_carrier(id.to_vec()) }

fn list(elements: Vec<Par>) -> Par { ground_elist_carrier(elements) }

fn tuple(elements: Vec<Par>) -> Par { ground_etuple_carrier(elements) }

fn set_map(entries: Vec<Par>) -> Par {
    expr_carrier(ExprInstance::EPathmapBody(EPathMap::new(
        entries,
        Vec::new(),
        false,
        None,
    )))
}

fn value_map(entries: Vec<(Par, Par)>) -> Par {
    expr_carrier(ExprInstance::EPathmapBody(EPathMap::new_map(
        entries,
        Vec::new(),
        false,
        None,
    )))
}

fn list_with_metadata(
    elements: Vec<Par>,
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
) -> Par {
    expr_carrier(ExprInstance::EListBody(EList {
        ps: elements,
        locally_free,
        connective_used,
        remainder,
    }))
}

fn nil() -> Par { Par::default() }

fn bound_variable() -> Par {
    expr_carrier(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        }),
    }))
}

fn ground_set() -> Par {
    expr_carrier(ExprInstance::ESetBody(ESet {
        ps: vec![gint(1)],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn two_exprs() -> Par {
    Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        },
        Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        },
    ])
}

fn duplicated_expr() -> Par {
    Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GInt(3)),
        };
        2
    ])
}

fn send() -> Par { Par::default().with_sends(vec![crate::rhoapi::Send::default()]) }

fn list_expression(elements: Vec<Par>) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: elements,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }
}

fn private_and_expr() -> Par {
    gprivate(b"mix").with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(3)),
    }])
}

fn deploy_id() -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![1, 2] })),
    }])
}

fn negation() -> Par { expr_carrier(ExprInstance::ENotBody(ENot { p: Some(gint(1)) })) }

fn unstable_leaf() -> Par {
    Par::default().with_connectives(vec![Connective {
        connective_instance: None,
    }])
}

fn wildcard() -> Var {
    Var {
        var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
    }
}

fn scalars() -> Vec<(String, Par)> {
    let mut scalars = Vec::with_capacity(48);
    scalars.push(labelled("false", gbool(false)));
    scalars.push(labelled("true", gbool(true)));
    for value in [0, 1, -1, 63, 64, -64, -65, 300, i64::MIN, i64::MAX] {
        scalars.push(labelled(format!("int {value}"), gint(value)));
    }
    let long_text = "λ".repeat(70);
    for text in ["", "a", "a\0λ", "\0", long_text.as_str()] {
        scalars.push(labelled(format!("string {text:?}"), gstring(text)));
    }
    for text in ["", "rho:test"] {
        scalars.push(labelled(format!("uri {text:?}"), guri(text)));
    }
    let long_bytes: Vec<u8> = (0..300).map(|index| (index % 256) as u8).collect();
    for bytes in [&[][..], &[0, 255][..], &long_bytes[..]] {
        scalars.push(labelled(
            format!("bytes of length {}", bytes.len()),
            gbytes(bytes),
        ));
    }
    for bits in [
        0,
        (-0.0f64).to_bits(),
        1.5f64.to_bits(),
        f64::NAN.to_bits(),
        u64::MAX,
        0x0102_0304_0506_0708,
    ] {
        scalars.push(labelled(
            format!("double bits {bits:#x}"),
            gdouble_bits(bits),
        ));
    }
    for bytes in [&[][..], &[0, 0, 1][..], &[0xFF][..]] {
        scalars.push(labelled(format!("big int {bytes:?}"), gbig_int(bytes)));
    }
    for (numerator, denominator) in [
        (&[][..], &[][..]),
        (&[0, 1][..], &[0, 2][..]),
        (&[][..], &[5][..]),
        (&[5][..], &[][..]),
    ] {
        scalars.push(labelled(
            format!("big rational {numerator:?}/{denominator:?}"),
            gbig_rat(numerator, denominator),
        ));
    }
    for (unscaled, scale) in [
        (&[][..], 0),
        (&[0, 1][..], u32::MAX),
        (&[][..], 7),
        (&[3][..], 0),
    ] {
        scalars.push(labelled(
            format!("fixed point {unscaled:?} scale {scale}"),
            gfixed_point(unscaled, scale),
        ));
    }
    for id in [&[][..], &[0, 255, 7][..], &[0xA5; 32][..]] {
        scalars.push(labelled(format!("private {id:?}"), gprivate(id)));
    }
    scalars
}

fn unstable_shapes() -> Vec<(String, Par)> {
    vec![
        labelled("nil", nil()),
        labelled("bound variable", bound_variable()),
        labelled("ground set", ground_set()),
        labelled("two expressions", two_exprs()),
        labelled("one expression twice", duplicated_expr()),
        labelled("send", send()),
        labelled("private and expression", private_and_expr()),
        labelled("deploy id", deploy_id()),
        labelled("negation", negation()),
        labelled("connective", unstable_leaf()),
        labelled("connective used", gint(9).with_connective_used(true)),
        labelled("locally free", gint(9).with_locally_free(vec![1])),
        labelled(
            "list with locally free",
            list_with_metadata(vec![gint(1)], vec![1], false, None),
        ),
        labelled(
            "list with connective used",
            list_with_metadata(vec![gint(1)], Vec::new(), true, None),
        ),
        labelled(
            "list with remainder",
            list_with_metadata(vec![gint(1)], Vec::new(), false, Some(wildcard())),
        ),
        labelled(
            "unstable map",
            expr_carrier(ExprInstance::EPathmapBody(EPathMap::new(
                vec![gint(1), gint(2)],
                Vec::new(),
                true,
                Some(wildcard()),
            ))),
        ),
        labelled(
            "ground list root with locally free",
            list(vec![gint(1), gstring("a")]).with_locally_free(vec![1]),
        ),
        labelled(
            "ground list root with connective used",
            list(vec![gint(1), gstring("a")]).with_connective_used(true),
        ),
        labelled(
            "ground list root beside a send",
            list(vec![gint(1), gstring("a")]).with_sends(vec![crate::rhoapi::Send::default()]),
        ),
        labelled(
            "ground list expression twice",
            Par::default().with_exprs(vec![list_expression(vec![gint(1), gstring("a")]); 2]),
        ),
        labelled(
            "ground list beside a private name",
            gprivate(&[4; 4]).with_exprs(vec![list_expression(vec![gint(1)])]),
        ),
    ]
}

fn generated_pars() -> Vec<(String, Par)> {
    let scalars = scalars();
    let unstable = unstable_shapes();
    let scalar_values: Vec<Par> = scalars.iter().map(|(_, par)| par.clone()).collect();
    let mut pars = Vec::with_capacity(scalars.len() * 4 + unstable.len() * 3 + 128);
    for (label, par) in &scalars {
        pars.push(labelled(format!("scalar {label}"), par.clone()));
        pars.push(labelled(
            format!("list of {label}"),
            list(vec![par.clone()]),
        ));
        pars.push(labelled(
            format!("tuple of {label}"),
            tuple(vec![par.clone()]),
        ));
        pars.push(labelled(
            format!("nested list of {label}"),
            list(vec![
                list(vec![par.clone()]),
                tuple(vec![par.clone(), gint(1)]),
            ]),
        ));
    }
    pars.push(labelled(
        "list of every scalar",
        list(scalar_values.clone()),
    ));
    pars.push(labelled(
        "tuple of every scalar",
        tuple(scalar_values.clone()),
    ));
    pars.push(labelled("empty list", list(Vec::new())));
    pars.push(labelled("empty tuple", tuple(Vec::new())));
    pars.push(labelled("list of empty list", list(vec![list(Vec::new())])));
    pars.push(labelled(
        "list of empty collections",
        list(vec![list(Vec::new()), tuple(Vec::new())]),
    ));
    pars.push(labelled(
        "tuple of lists",
        tuple(vec![list(vec![gint(1)]), list(vec![gstring("x"), gint(2)])]),
    ));
    pars.push(labelled(
        "unary list spine around a string",
        (0..64).fold(gstring("leaf"), |inner, _| list(vec![inner])),
    ));
    pars.push(labelled(
        "unary tuple spine around a private name",
        (0..64).fold(gprivate(&[9; 32]), |inner, _| tuple(vec![inner])),
    ));
    pars.push(labelled(
        "deep binary list",
        (0..128).fold(gint(0), |inner, level| list(vec![inner, gint(level)])),
    ));
    pars.push(labelled(
        "deep binary tuple",
        (0..128).fold(gint(0), |inner, level| {
            tuple(vec![gstring(SHORT_STRINGS[level % 16]), inner])
        }),
    ));
    pars.push(labelled(
        "deep alternating tuple and set map",
        (0..24).fold(gint(0), |inner, level| {
            if level % 2 == 0 {
                tuple(vec![inner, gint(level as i64)])
            } else {
                set_map(vec![inner])
            }
        }),
    ));
    pars.push(labelled("wide list", list((0..512).map(gint).collect())));
    pars.push(labelled(
        "wide tuple",
        tuple(
            (0..300)
                .map(|index| gstring(SHORT_STRINGS[index % 16]))
                .collect(),
        ),
    ));
    pars.push(labelled(
        "large cell shape",
        list(
            (0..16)
                .map(|index| {
                    tuple(vec![
                        gprivate(&[index as u8; 32]),
                        gstring(SHORT_STRINGS[index]),
                        gint(index as i64),
                    ])
                })
                .collect(),
        ),
    ));
    pars.push(labelled(
        "list of sixty-four tuples",
        list(
            (0..64)
                .map(|index| {
                    tuple(vec![
                        gprivate(&[index as u8; 32]),
                        gstring(SHORT_STRINGS[index % 16]),
                    ])
                })
                .collect(),
        ),
    ));
    let set_entries = vec![gint(3), gstring("k"), list(vec![gint(1), gint(2)])];
    let map_entries = vec![
        (gstring("k"), gint(7)),
        (gint(1), list(vec![gint(2)])),
        (
            tuple(vec![gprivate(&[1; 4]), gint(5)]),
            set_map(vec![gint(6)]),
        ),
    ];
    for (label, par) in set_entries
        .iter()
        .map(|entry| labelled("set map entry", entry.clone()))
        .chain(map_entries.iter().flat_map(|(key, value)| {
            [
                labelled("value map key", key.clone()),
                labelled("value map value", value.clone()),
            ]
        }))
    {
        pars.push((label, par));
    }
    let nested_set = set_map(set_entries.clone());
    let nested_value_map = value_map(map_entries.clone());
    pars.push(labelled("set map root", nested_set.clone()));
    pars.push(labelled("value map root", nested_value_map.clone()));
    pars.push(labelled(
        "empty map root",
        expr_carrier(ExprInstance::EPathmapBody(EPathMap::default())),
    ));
    pars.push(labelled(
        "tuple holding a set map",
        tuple(vec![gprivate(&[2; 8]), nested_set.clone()]),
    ));
    pars.push(labelled(
        "list holding a value map",
        list(vec![nested_value_map.clone()]),
    ));
    pars.push(labelled(
        "list whose last element is a map",
        list(vec![gint(1), gint(2), nested_set.clone()]),
    ));
    pars.push(labelled(
        "tuple whose last element is an empty map",
        tuple(vec![
            gstring("x"),
            expr_carrier(ExprInstance::EPathmapBody(EPathMap::default())),
        ]),
    ));
    pars.push(labelled(
        "map inside a map inside a list",
        list(vec![set_map(vec![nested_set.clone(), gint(4)])]),
    ));
    pars.push(labelled(
        "split list of mixed collections",
        list(vec![
            gprivate(&[3; 32]),
            tuple(vec![gstring("a"), gint(1)]),
            list(vec![gbool(true)]),
        ]),
    ));
    for (label, par) in &unstable {
        pars.push(labelled(format!("unstable {label}"), par.clone()));
        pars.push(labelled(
            format!("split list holding {label}"),
            list(vec![gint(1), par.clone(), gstring("z")]),
        ));
        pars.push(labelled(
            format!("tuple holding {label}"),
            tuple(vec![par.clone()]),
        ));
    }
    pars.push(labelled(
        "split list of unstable shapes only",
        list(unstable.iter().map(|(_, par)| par.clone()).collect()),
    ));
    pars
}

fn deep_unstable(wrappers: usize) -> Par {
    (0..wrappers).fold(unstable_leaf(), |inner, _| tuple(vec![inner]))
}

fn deep_stable(wrappers: usize) -> Par {
    (0..wrappers).fold(gint(7), |inner, _| tuple(vec![inner]))
}

fn nested_map_key_chain(depth: usize) -> Par {
    (0..depth).fold(gint(0), |key, _| value_map(vec![(key, gint(0))]))
}

fn existing_trie_path_inputs() -> Vec<(String, Par)> {
    let mut pars = Vec::with_capacity(128);
    for wrappers in [0, 1, 8, 20, 33, 34, 64] {
        pars.push(labelled(
            format!("deep unstable {wrappers}"),
            deep_unstable(wrappers),
        ));
    }
    for wrappers in [0, 1, 8, 20, 33, 34, 64, 256] {
        pars.push(labelled(
            format!("deep stable {wrappers}"),
            deep_stable(wrappers),
        ));
    }
    pars.push(labelled(
        "escape-arm map carrier",
        expr_carrier(ExprInstance::EPathmapBody(EPathMap::new(
            vec![gint(1), gint(2)],
            Vec::new(),
            true,
            Some(wildcard()),
        ))),
    ));
    for depth in [1, 2, 4, 8, 16, 32] {
        pars.push(labelled(
            format!("nested map key chain {depth}"),
            nested_map_key_chain(depth),
        ));
    }
    let worklist_key = Par::default()
        .with_locally_free(vec![0b101])
        .with_connective_used(true);
    let worklist_value = two_exprs().with_locally_free(vec![0b1010]);
    let split_key = list(vec![gint(3), nil()]);
    pars.push(labelled("worklist split key", split_key.clone()));
    pars.push(labelled("worklist key", worklist_key.clone()));
    pars.push(labelled("worklist value", worklist_value.clone()));
    pars.push(labelled(
        "worklist map",
        value_map(vec![(worklist_key, worklist_value), (split_key, gint(4))]),
    ));
    pars.push(labelled("two nils", list(vec![nil(), nil()])));
    for depth in [0, 1, 4, 32, 1024] {
        pars.push(labelled(
            format!("wrapped unary list {depth}"),
            Par::wrap_unary_elist(depth, gint(0))
                .expect("unary list fixture depth is representable"),
        ));
    }
    for (label, par) in [
        ("golden 1", gint(1)),
        ("golden [1]", list(vec![gint(1)])),
        ("golden a", gstring("a")),
        ("golden [a, x]", list(vec![gstring("a"), gstring("x")])),
        ("golden [a, 1]", list(vec![gstring("a"), gint(1)])),
        ("alphabet -7", gint(-7)),
        ("alphabet empty string", gstring("")),
        ("alphabet []", list(Vec::new())),
        ("alphabet [a]", list(vec![gstring("a")])),
        (
            "alphabet [[2], q]",
            list(vec![list(vec![gint(2)]), gstring("q")]),
        ),
        (
            "books",
            list(vec![
                gstring("books"),
                gstring("fiction"),
                gstring("gatsby"),
            ]),
        ),
        ("seven", gint(7)),
        ("nul string", gstring("\0")),
        ("[7]", list(vec![gint(7)])),
        ("[a, nul]", list(vec![gstring("a"), gstring("\0")])),
        ("query walk 2", gint(2)),
        ("query walk 3", gint(3)),
        (
            "epm1 snapshot map",
            value_map(vec![(gstring("key"), gint(7))]),
        ),
        ("legacy int min", gint(i64::MIN)),
        ("legacy int max", gint(i64::MAX)),
        ("legacy false", gbool(false)),
        ("legacy true", gbool(true)),
        ("legacy string", gstring("a\0λ")),
        ("legacy uri", guri("rho:test")),
        ("legacy bytes", gbytes(&[0, 255])),
        ("legacy big int", gbig_int(&[0, 0, 1])),
        ("legacy double", gdouble_bits(u64::MAX)),
        ("legacy rational", gbig_rat(&[0, 1], &[0, 2])),
        ("legacy fixed point", gfixed_point(&[0, 1], u32::MAX)),
        ("legacy tuple", tuple(vec![gint(1), gint(2)])),
        ("legacy private", gprivate(&[0, 255, 7])),
        ("legacy [nil]", list(vec![nil()])),
        (
            "legacy [nil] list locally free",
            list_with_metadata(vec![nil()], vec![1], false, None),
        ),
        (
            "legacy [nil] list connective used",
            list_with_metadata(vec![nil()], Vec::new(), true, None),
        ),
        (
            "legacy [nil] list remainder",
            list_with_metadata(vec![nil()], Vec::new(), false, Some(Var::default())),
        ),
        (
            "legacy [nil] root locally free",
            list(vec![nil()]).with_locally_free(vec![1]),
        ),
        (
            "legacy [nil] root connective used",
            list(vec![nil()]).with_connective_used(true),
        ),
        (
            "legacy 32-entry map",
            value_map((0..32).map(|key| (gint(key), gint(key + 1))).collect()),
        ),
        ("legacy set", set_map(vec![gint(1), gint(2)])),
        (
            "legacy [d, 2]",
            list(vec![
                nil().with_locally_free(vec![2]).with_connective_used(true),
                gint(2),
            ]),
        ),
        (
            "legacy [2, d]",
            list(vec![
                gint(2),
                nil().with_locally_free(vec![2]).with_connective_used(true),
            ]),
        ),
        ("construction string", gstring("a\0b")),
        ("construction bytes", gbytes(&[0, 255, 1])),
        ("construction [1, -2]", list(vec![gint(1), gint(-2)])),
        ("construction tuple", tuple(vec![gint(3), gint(4)])),
        ("construction private", gprivate(&[0, 1, 255])),
        ("construction negation", negation()),
        (
            "construction nested sets",
            set_map(vec![set_map(vec![gint(7)]), gint(9)]),
        ),
        ("construction map", value_map(vec![(gint(1), gint(2))])),
        (
            "construction map of set",
            value_map(vec![
                (gint(1), set_map(vec![gint(1), gint(2)])),
                (gint(2), gint(3)),
            ]),
        ),
    ] {
        pars.push(labelled(label, par));
    }
    pars
}

fn corpus_pars() -> Vec<(String, Par)> {
    let mut pars = par_corpus::par_corpus();
    for (label, list_with_random) in par_corpus::list_par_with_random_corpus() {
        for (index, par) in list_with_random.pars.into_iter().enumerate() {
            pars.push(labelled(
                format!("list_par_with_random::{label}::{index}"),
                par,
            ));
        }
    }
    for (label, pattern) in par_corpus::bind_pattern_corpus() {
        for (index, par) in pattern.patterns.into_iter().enumerate() {
            pars.push(labelled(format!("bind_pattern::{label}::{index}"), par));
        }
    }
    for (label, continuation) in par_corpus::tagged_continuation_corpus() {
        if let Some(guard) = continuation.guard {
            pars.push(labelled(
                format!("tagged_continuation::{label}::guard"),
                guard,
            ));
        }
        if let Some(TaggedCont::ParBody(with_random)) = continuation.tagged_cont {
            if let Some(body) = with_random.body {
                pars.push(labelled(
                    format!("tagged_continuation::{label}::body"),
                    body,
                ));
            }
        }
    }
    for (label, with_random) in par_corpus::par_with_random_corpus() {
        if let Some(body) = with_random.body {
            pars.push(labelled(format!("par_with_random::{label}"), body));
        }
    }
    for (label, bind_patterns) in par_corpus::list_bind_patterns_corpus() {
        for (pattern_index, pattern) in bind_patterns.patterns.into_iter().enumerate() {
            for (index, par) in pattern.patterns.into_iter().enumerate() {
                pars.push(labelled(
                    format!("list_bind_patterns::{label}::{pattern_index}::{index}"),
                    par,
                ));
            }
        }
    }
    pars.push(labelled(
        "cost_annotated_par",
        par_corpus::cost_annotated_par(),
    ));
    pars.push(labelled(
        "ground_pathmap",
        expr_carrier(ExprInstance::EPathmapBody(par_corpus::ground_pathmap())),
    ));
    pars.push(labelled(
        "nonground_pathmap",
        expr_carrier(ExprInstance::EPathmapBody(par_corpus::nonground_pathmap())),
    ));
    for tag_value in [0u8, 1, 0x7F, 0xFF] {
        pars.push(labelled(
            format!("tagged::{tag_value}"),
            par_corpus::tagged(tag_value),
        ));
    }
    for depth in [0, 3, 64, 255] {
        pars.push(labelled(
            format!("deep_par::{depth}"),
            par_corpus::deep_par(depth),
        ));
    }
    for depth in [3, 9, 17, 33] {
        pars.push(labelled(
            format!("deep_mixed_par::{depth}"),
            par_corpus::deep_mixed_par(depth),
        ));
    }
    pars
}

#[test]
fn stored_byte_encoder_matches_frozen_encoder_on_corpus_pars() {
    let pars = corpus_pars();
    assert!(pars.len() > 50, "the corpus holds {} Pars", pars.len());
    for (label, par) in &pars {
        assert_matches_frozen_encoder(label, par);
    }
}

#[test]
fn stored_byte_encoder_matches_frozen_encoder_on_generated_pars() {
    let pars = generated_pars();
    assert!(pars.len() > 200, "the generator built {} Pars", pars.len());
    for (label, par) in &pars {
        assert_matches_frozen_encoder(label, par);
    }
}

#[test]
fn stored_byte_encoder_matches_frozen_encoder_on_existing_trie_path_inputs() {
    for (label, par) in &existing_trie_path_inputs() {
        assert_matches_frozen_encoder(label, par);
    }
    for (par, golden) in [
        (gint(1), &[0x03, 0x02][..]),
        (list(vec![gint(1)]), &[0x03, 0x02, 0x00][..]),
        (gstring("a"), &[0x04, 0x01, 0x61][..]),
        (
            list(vec![gstring("a"), gstring("x")]),
            &[0x04, 0x01, 0x61, 0x04, 0x01, 0x78, 0x00][..],
        ),
        (
            list(vec![gstring("a"), gint(1)]),
            &[0x04, 0x01, 0x61, 0x03, 0x02, 0x00][..],
        ),
    ] {
        assert_eq!(encode_trie_path(&par), golden);
    }
}

fn length_delimited(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    encode_key(number, WireType::LengthDelimited, &mut out);
    encode_varint(payload.len() as u64, &mut out);
    out.extend_from_slice(payload);
    out
}

fn varint_field(number: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    encode_key(number, WireType::Varint, &mut out);
    encode_varint(value, &mut out);
    out
}

fn fixed64_field(number: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    encode_key(number, WireType::SixtyFourBit, &mut out);
    out.extend_from_slice(&value.to_le_bytes());
    out
}

fn translate_stored(component: StoredComponent<'_>, split_top_level_list: bool) -> (bool, Vec<u8>) {
    let mut machine = EncMachine::new();
    machine.root.push(0xEE);
    let translated = machine
        .emit_stored_stable_component(component, split_top_level_list)
        .expect("the root buffer accepts raw bytes");
    (translated, machine.root)
}

#[test]
fn stored_byte_translator_refuses_non_canonical_payloads_and_restores_the_buffer() {
    let gint_one = varint_field(2, 2);
    let gint_minus_one = varint_field(2, 1);
    let bool_true = varint_field(1, 1);
    let double = fixed64_field(34, 0x0102_0304_0506_0708);
    let gint_one_par = length_delimited(5, &gint_one);
    let string_a = length_delimited(3, b"a");
    let string_a_par = length_delimited(5, &string_a);
    let split_list_of_a = length_delimited(20, &length_delimited(1, &string_a_par));
    let private_body = length_delimited(1, &length_delimited(1, &[7, 8]));
    let empty_private_body = length_delimited(1, &[]);
    let accepted: Vec<(&str, StoredComponent<'_>, bool, Vec<u8>)> = vec![
        ("int one", StoredComponent::Expr(&gint_one), false, vec![
            0xEE, 0x03, 0x02,
        ]),
        (
            "int minus one",
            StoredComponent::Expr(&gint_minus_one),
            false,
            vec![0xEE, 0x03, 0x01],
        ),
        ("bool true", StoredComponent::Expr(&bool_true), false, vec![
            0xEE, 0x02,
        ]),
        (
            "split list of a",
            StoredComponent::Expr(&split_list_of_a),
            true,
            vec![0xEE, 0x04, 0x01, 0x61, 0x00],
        ),
        (
            "bare list of a",
            StoredComponent::Expr(&split_list_of_a),
            false,
            vec![0xEE, 0x0B, 0x01, 0x04, 0x01, 0x61],
        ),
        ("double", StoredComponent::Expr(&double), false, vec![
            0xEE, 0x07, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        ]),
        (
            "private name",
            StoredComponent::Unforgeable(&private_body),
            false,
            vec![0xEE, 0x0E, 0x02, 0x07, 0x08],
        ),
        (
            "empty private name",
            StoredComponent::Unforgeable(&empty_private_body),
            false,
            vec![0xEE, 0x0E, 0x00],
        ),
    ];
    for (label, component, split_top_level_list, expected) in accepted {
        assert_eq!(
            translate_stored(component, split_top_level_list),
            (true, expected),
            "accepted payload {label}"
        );
    }
    let refused: Vec<(&str, Vec<u8>, bool)> = vec![
        ("bool varint two", varint_field(1, 2), false),
        (
            "two oneof fields",
            [varint_field(1, 1), varint_field(2, 2)].concat(),
            false,
        ),
        ("unknown expression field", varint_field(99, 1), false),
        ("invalid utf-8 string", length_delimited(3, &[0xFF]), false),
        ("invalid utf-8 uri", length_delimited(4, &[0xC3]), false),
        (
            "explicit empty numerator",
            length_delimited(36, &length_delimited(1, &[])),
            false,
        ),
        (
            "decreasing rational fields",
            length_delimited(
                36,
                &[length_delimited(2, &[5]), length_delimited(1, &[3])].concat(),
            ),
            false,
        ),
        (
            "explicit zero scale",
            length_delimited(37, &varint_field(2, 0)),
            false,
        ),
        (
            "scale beyond u32",
            length_delimited(37, &varint_field(2, 1 << 32)),
            false,
        ),
        (
            "list with locally free",
            length_delimited(
                20,
                &[
                    length_delimited(1, &gint_one_par),
                    length_delimited(3, &[1]),
                ]
                .concat(),
            ),
            false,
        ),
        (
            "list child with two fields",
            length_delimited(
                20,
                &length_delimited(1, &[gint_one_par.clone(), varint_field(10, 1)].concat()),
            ),
            false,
        ),
        (
            "nil list child",
            length_delimited(20, &length_delimited(1, &[])),
            false,
        ),
        ("map", length_delimited(32, &[]), false),
        ("truncated string", vec![0x1A, 0x05, 0x61], false),
        ("non-minimal varint", vec![0x10, 0x82, 0x00], false),
        (
            "split list then a refused child",
            length_delimited(
                20,
                &[
                    length_delimited(1, &gint_one_par),
                    length_delimited(1, &length_delimited(5, &varint_field(1, 7))),
                ]
                .concat(),
            ),
            true,
        ),
        (
            "tuple then a nil child",
            length_delimited(
                21,
                &[length_delimited(1, &string_a_par), length_delimited(1, &[])].concat(),
            ),
            false,
        ),
    ];
    for (label, expr, split_top_level_list) in &refused {
        assert_eq!(
            translate_stored(StoredComponent::Expr(expr), *split_top_level_list),
            (false, vec![0xEE]),
            "refused payload {label}"
        );
    }
    for (label, unforgeable) in [
        ("deploy id", length_delimited(2, &[])),
        (
            "explicit empty private id",
            length_delimited(1, &length_delimited(1, &[])),
        ),
        (
            "private with an unknown field",
            length_delimited(1, &varint_field(2, 1)),
        ),
    ] {
        assert_eq!(
            translate_stored(StoredComponent::Unforgeable(&unforgeable), false),
            (false, vec![0xEE]),
            "refused unforgeable {label}"
        );
    }
}

fn stable_scalar() -> impl Strategy<Value = Par> {
    prop_oneof![
        any::<bool>().prop_map(gbool),
        any::<i64>().prop_map(gint),
        "[a-z0-9λ]{0,12}".prop_map(|text| gstring(&text)),
        "[a-z:]{0,8}".prop_map(|text| guri(&text)),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(|bytes| gbytes(&bytes)),
        any::<u64>().prop_map(gdouble_bits),
        proptest::collection::vec(any::<u8>(), 0..8).prop_map(|bytes| gbig_int(&bytes)),
        (
            proptest::collection::vec(any::<u8>(), 0..6),
            proptest::collection::vec(any::<u8>(), 0..6)
        )
            .prop_map(|(numerator, denominator)| gbig_rat(&numerator, &denominator)),
        (proptest::collection::vec(any::<u8>(), 0..6), any::<u32>())
            .prop_map(|(unscaled, scale)| gfixed_point(&unscaled, scale)),
        proptest::collection::vec(any::<u8>(), 0..33).prop_map(|id| gprivate(&id)),
    ]
}

fn stable_tree() -> impl Strategy<Value = Par> {
    stable_scalar().prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(list),
            proptest::collection::vec(inner.clone(), 0..4).prop_map(tuple),
            proptest::collection::vec(inner.clone(), 0..3).prop_map(set_map),
            proptest::collection::vec((inner.clone(), inner), 0..3).prop_map(value_map),
        ]
    })
}

fn unstable_tree() -> impl Strategy<Value = Par> {
    prop_oneof![
        Just(nil()),
        Just(bound_variable()),
        Just(ground_set()),
        Just(two_exprs()),
        Just(duplicated_expr()),
        Just(send()),
        Just(private_and_expr()),
        Just(deploy_id()),
        Just(negation()),
        Just(unstable_leaf()),
        stable_tree().prop_map(|par| par.with_locally_free(vec![1])),
        stable_tree().prop_map(|par| par.with_connective_used(true)),
    ]
}

fn mixed_tree() -> impl Strategy<Value = Par> {
    prop_oneof![
        3 => stable_tree(),
        1 => unstable_tree(),
        2 => proptest::collection::vec(
            prop_oneof![3 => stable_tree(), 1 => unstable_tree()],
            0..5
        )
        .prop_map(list),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_stable_pars_match_the_frozen_encoder(par in stable_tree()) {
        assert_matches_frozen_encoder("random stable par", &par);
    }

    #[test]
    fn random_pars_and_split_lists_match_the_frozen_encoder(par in mixed_tree()) {
        assert_matches_frozen_encoder("random par", &par);
    }
}
