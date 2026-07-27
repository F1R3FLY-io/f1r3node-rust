//! The pretty printer's **recursive oracle twin**, with CHECKED provenance.
//!
//! [`PrettyPrinter`]'s traversal is an explicit pushdown driver
//! (`pretty_printer::drive`). Every function in this file is the body that
//! driver replaced, kept so the two can be differentiated
//! (`pretty_printer::differential`). Its whole evidential value rests on being a
//! faithful copy of the pre-conversion code: **if the oracle drifts toward the
//! machine, the differential compares the machine against something that has
//! been adjusted to agree with it, and every green result becomes worthless
//! without anyone noticing.**
//!
//! # Why this is its own file
//!
//! Two reasons, and the second is the load-bearing one.
//!
//! 1. Uniformity: every other oracle in this workspace already lives in its own
//!    file (`substitute_oracle.rs`, `normalize_recursive.rs`).
//! 2. `rustfmt.toml`'s `ignore` is FILE-level. Text that asserts byte-identity
//!    against an external source must not be silently rewritten by a formatter —
//!    but while this code sat inside `pretty_printer.rs`, excluding it would have
//!    frozen 4,000 lines that make no such claim. `#[rustfmt::skip]` is refused
//!    for the reason it was refused before: it edits the text whose byte-identity
//!    is the point. Extraction is what makes a file-level exclusion honest, and
//!    `2fee95d8`'s criterion — "a file earns a listing only while its own text
//!    asserts provenance against an external source" — is then satisfied by this
//!    file and by no other part of the printer.
//!
//! # ⚠ What the previous claim said, and what is actually true
//!
//! The banner this documentation replaces read:
//!
//! > Every function below is the PRE-CONVERSION body, kept verbatim, with
//! > exactly one class of change: its calls to the printer's own mutually
//! > recursive entry points are renamed to the `oracle_` twins ... **Nothing
//! > else is edited — not a format string, not an argument order, not a
//! > comment.**
//!
//! It cited nothing: no commit, no path, no line range. Writing the checker is
//! what established the truth, and the claim was WRONG — the same way, and for
//! the same reason, that `779bf881` found the normalizer oracle's citations
//! wrong. Measured against `739368a4` (the commit before `6675fc06`, the
//! conversion):
//!
//! | | functions | pre-conversion lines |
//! |---|---|---|
//! | byte-identical under the declared rename | **4 of 10** | 517 |
//! | carrying at least one declared deviation | **6 of 10** | 454 |
//!
//! Three of the four public wrappers had their explanatory comments DELETED —
//! precisely the "not a comment" the banner promised — and all four had their
//! visibility narrowed. Those are legitimate edits; what was not legitimate was
//! a blanket "verbatim" that no reader could check.
//!
//! # The declared transformation, as DATA
//!
//! Everything below is re-derived from git by
//! `rholang/tests/normalize_oracle_provenance.rs`, which applies these steps in
//! order and then compares byte for byte. Anything a maintainer changes here
//! that is not one of them makes that test red. That is the point.
//!
//! | step | why |
//! |---|---|
//! | every mutually-recursive entry point gains an `oracle_` / `_oracle_` prefix | so the twin is a CLOSED recursive system and cannot silently re-enter the machine it is checking |
//! | the four wrappers become `pub(crate)` | the oracle exports nothing publicly; the differential is in-crate |
//! | per-block `DECLARED_DEVIATIONS` | every remaining difference, spelled out with its reason |
//!
//! # The true LEAVES are deliberately SHARED with production
//!
//! `build_string_from_var`, `build_string_from_unforgeable`,
//! `build_remainder_string`, `build_variables`, `cap`, `indent_string`,
//! `bound_id`, `set_base_id`, `is_empty_par`, `is_new_var`,
//! `twos_complement_to_decimal`. The conversion did not touch them, so
//! duplicating them would test the copy rather than the driving.
//!
//! `build_pattern` and `build_match_case` exist ONLY here now: their machine
//! forms are `PpWork::RecvBindStep` + `PpKont::RecvBindJoin` and
//! `PpWork::CaseStep` + `PpKont::CaseJoin`. They are not commented out — they
//! are live and executed by every differential run.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus, EMinusMinus, EMod, EMult,
    ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, ETuple, EVar, Expr, MatchCase, Par,
};
use models::rust::bundle_ops::BundleOps;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use shared::rust::shared::string_ops::wrap_with_braces;

use super::errors::InterpreterError;
use super::pretty_printer::{
    twos_complement_to_decimal, AsPpNode, NewBindRange, PpNode, PrettyPrinter,
};

impl PrettyPrinter {
    /// Twin of [`PrettyPrinter::build_string_from_expr`].
    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 153-163, at commit 739368a4
    // ===================================================================
    pub(crate) fn oracle_build_string_from_expr(&mut self, e: &Expr) -> String {
        match self._oracle_build_string_from_expr(e) {
            Ok(str) => self.cap(&str),
            Err(err) => format!("<unprintable expr: {}>", err),
        }
    }

    /// Twin of [`PrettyPrinter::build_string_from_message`].
    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 173-175, at commit 739368a4
    // ===================================================================
    pub(crate) fn oracle_build_string_from_message<T: AsPpNode + ?Sized>(
        &mut self,
        m: &T,
    ) -> String {
        self.oracle_build_string_from_node(m.as_pp_node())
    }

    /// Twin of [`PrettyPrinter::build_string_from_node`].
    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 179-188, at commit 739368a4
    // ===================================================================
    pub(crate) fn oracle_build_string_from_node(&mut self, node: PpNode<'_>) -> String {
        match self._oracle_build_string_from_message(node, 0) {
            Ok(str) => self.cap(&str),
            Err(err) => format!("<unprintable: {}>", err),
        }
    }

    /// Twin of [`PrettyPrinter::build_channel_string`].
    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 190-200, at commit 739368a4
    // ===================================================================
    pub(crate) fn oracle_build_channel_string(&mut self, m: &Par) -> String {
        match self._oracle_build_channel_string(m, 0) {
            Ok(str) => self.cap(&str),
            Err(err) => format!("<unprintable channel: {}>", err),
        }
    }


    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 224-670, at commit 739368a4
    // ===================================================================
    fn _oracle_build_string_from_expr(&mut self, e: &Expr) -> Result<String, InterpreterError> {
        match &e.expr_instance {
            Some(instance) => match instance {
                ExprInstance::ENegBody(ENeg { p }) => Ok(format!(
                    "-{}",
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p.as_ref().expect("ENeg par field was None, should be Some")
                    ))
                )),

                ExprInstance::ENotBody(ENot { p }) => Ok(format!(
                    "~{}",
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p.as_ref().expect("ENot par field was None, should be Some")
                    ))
                )),

                ExprInstance::EMultBody(EMult { p1, p2 }) => Ok(format!(
                    "{} * {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EMult p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EMult p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EDivBody(EDiv { p1, p2 }) => Ok(format!(
                    "{} / {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EDiv p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EDiv p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EModBody(EMod { p1, p2 }) => Ok(format!(
                    "{} % {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EMod p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EMod p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => Ok(format!(
                    "{} %% {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EPercentPercent p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EPercentPercent p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EPlusBody(EPlus { p1, p2 }) => Ok(format!(
                    "{} + {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EPlus p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EPlus p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => Ok(format!(
                    "{} ++ {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EPlusPlus p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EPlusPlus p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EMinusBody(EMinus { p1, p2 }) => Ok(format!(
                    "{} - {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EMinus p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EMinus p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => Ok(format!(
                    "{} - {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref()
                            .expect("EMinusMinus p1 field was None, should be Some")
                    ),
                    wrap_with_braces(
                        self.oracle_build_string_from_message(
                            p2.as_ref()
                                .expect("EMinusMinus p2 field was None, should be Some")
                        )
                    )
                )),

                ExprInstance::EAndBody(EAnd { p1, p2 }) => Ok(format!(
                    "{} && {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EAnd p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EAnd p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EOrBody(EOr { p1, p2 }) => Ok(format!(
                    "{} || {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EOr p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EOr p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EEqBody(EEq { p1, p2 }) => Ok(format!(
                    "{} == {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EEq p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EEq p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::ENeqBody(ENeq { p1, p2 }) => Ok(format!(
                    "{} != {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("ENeq p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("ENeq p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EGtBody(EGt { p1, p2 }) => Ok(format!(
                    "{} > {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EGt p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EGt p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EGteBody(EGte { p1, p2 }) => Ok(format!(
                    "{} >= {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("EGte p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("EGte p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::ELtBody(ELt { p1, p2 }) => Ok(format!(
                    "{} < {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("ELt p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("ELt p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::ELteBody(ELte { p1, p2 }) => Ok(format!(
                    "{} <= {}",
                    self.oracle_build_string_from_message(
                        p1.as_ref().expect("ELte p1 field was None, should be Some")
                    ),
                    wrap_with_braces(self.oracle_build_string_from_message(
                        p2.as_ref().expect("ELte p2 field was None, should be Some")
                    ))
                )),

                ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
                    Ok(wrap_with_braces(format!(
                        "{} matches {}",
                        self.oracle_build_string_from_message(
                            target
                                .as_ref()
                                .expect("EMatches target field was None, should be Some")
                        ),
                        self.oracle_build_string_from_message(
                            pattern
                                .as_ref()
                                .expect("EMatches pattern field was None, should be Some")
                        )
                    )))
                }

                /*
                  I change this code, because list with remainder with always return comma after last element, like [x0, x1, 7,...free0]
                  However, in a conversation with Steven we decided that we should rely on Scala [x0, x1, 7...free0] (after last element we don't have comma)
                */
                // ExprInstance::EListBody(EList { ps, remainder, .. }) => Ok(format!(
                //     "[{},{}]",
                //     self.oracle_build_vec(ps),
                //     self.build_remainder_string(remainder)
                // )),
                ExprInstance::EListBody(EList { ps, remainder, .. }) => {
                    let elements = self.oracle_build_vec(ps);
                    let remainder_string = self.build_remainder_string(remainder);

                    let full_result = if remainder.is_some() && !elements.is_empty() {
                        format!("[{}{}]", elements, remainder_string)
                    } else if remainder.is_some() {
                        format!("[{}]", remainder_string)
                    } else {
                        format!("[{}]", elements)
                    };

                    Ok(full_result)
                }

                ExprInstance::ETupleBody(ETuple { ps, .. }) => {
                    Ok(format!("({})", self.oracle_build_vec(ps),))
                }

                ExprInstance::ESetBody(eset) => {
                    let par_set = ParSetTypeMapper::eset_to_par_set(eset.clone());
                    let pars = par_set.ps;
                    let remainder = &par_set.remainder;

                    //TODO same problem with comma

                    // Ok(format!(
                    //     "Set({},{})",
                    //     self.oracle_build_vec(&pars.sorted_pars),
                    //     self.build_remainder_string(remainder)
                    // ))

                    let elements = self.oracle_build_vec(&pars.sorted_pars);
                    let remainder_string = self.build_remainder_string(remainder);
                    let full_result = if remainder.is_some() && !elements.is_empty() {
                        format!("Set({}{})", elements, remainder_string)
                    } else if remainder.is_some() {
                        format!("Set({})", remainder_string)
                    } else {
                        format!("Set({})", elements)
                    };

                    Ok(full_result)
                }

                ExprInstance::EMapBody(emap) => {
                    let par_map = ParMapTypeMapper::emap_to_par_map(emap.clone());
                    let sorted_list = par_map.ps.sorted_list;
                    let remainder = &par_map.remainder;
                    let mut result = String::from("{");

                    for (i, (key, value)) in sorted_list.iter().enumerate() {
                        result.push_str(&self.oracle_build_string_from_message(key));
                        result.push_str(" : ");
                        result.push_str(&self.oracle_build_string_from_message(value));

                        if i != sorted_list.len() - 1 {
                            result.push_str(", ");
                        }
                    }

                    result.push_str(&self.build_remainder_string(remainder));
                    result.push('}');

                    Ok(result)
                }

                ExprInstance::EPathmapBody(pathmap) => {
                    // Similar to EListBody - print elements in pathmap syntax {| ... |}
                    let elements = self.oracle_build_vec(&pathmap.ps);
                    let remainder_string = self.build_remainder_string(&pathmap.remainder);

                    let full_result = if pathmap.remainder.is_some() && !elements.is_empty() {
                        format!("{{|{}{}|}}", elements, remainder_string)
                    } else if pathmap.remainder.is_some() {
                        format!("{{|{}|}}", remainder_string)
                    } else {
                        format!("{{|{}|}}", elements)
                    };

                    Ok(full_result)
                }

                ExprInstance::EZipperBody(zipper) => {
                    // Print zipper showing the underlying PathMap and current position
                    let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                    let elements = self.oracle_build_vec(&pathmap.ps);
                    let remainder_string = self.build_remainder_string(&pathmap.remainder);
                    let zipper_type = if zipper.is_write_zipper {
                        "WriteZipper"
                    } else {
                        "ReadZipper"
                    };

                    let pathmap_repr = if pathmap.remainder.is_some() && !elements.is_empty() {
                        format!("{{|{}{}|}}", elements, remainder_string)
                    } else if pathmap.remainder.is_some() {
                        format!("{{|{}|}}", remainder_string)
                    } else {
                        format!("{{|{}|}}", elements)
                    };

                    // Format current_path as a readable list. W2b-1: each
                    // segment is a codec `encode_trie_segment(element)`; frame
                    // it as a 1-element split path (`seg ∥ 0x00`) to recover
                    // the element Par faithfully (ANY eval_stable element, vs
                    // the former lossy GString-only SExpr decode) and render
                    // it. Zipper display strings move accordingly (re-pinned).
                    let current_path_repr = if zipper.current_path.is_empty() {
                        "[]".to_string()
                    } else {
                        use models::rust::canonical_path::{decode_trie_path, tag};

                        let mut path_segments: Vec<String> =
                            Vec::with_capacity(zipper.current_path.len());
                        for segment in &zipper.current_path {
                            let mut framed = segment.clone();
                            framed.push(tag::TERM);
                            let rendered = match decode_trie_path(&framed) {
                                Ok(par) => match par
                                    .exprs
                                    .first()
                                    .and_then(|ex| ex.expr_instance.as_ref())
                                {
                                    Some(ExprInstance::EListBody(list))
                                        if !list.ps.is_empty() =>
                                    {
                                        self.oracle_build_channel_string(&list.ps[0])
                                    }
                                    _ => format!("0x{}", hex::encode(segment)),
                                },
                                Err(_) => format!("0x{}", hex::encode(segment)),
                            };
                            path_segments.push(rendered);
                        }
                        format!("[{}]", path_segments.join(", "))
                    };

                    // Format: ReadZipper(at: ["books", "fiction"], {| ... |})
                    Ok(format!(
                        "{}(at: {}, {})",
                        zipper_type, current_path_repr, pathmap_repr
                    ))
                }

                ExprInstance::EVarBody(EVar { v }) => Ok(self.build_string_from_var(
                    v.as_ref()
                        .expect("var field on EVar was None, should be Some"),
                )),

                ExprInstance::GBool(b) => Ok(b.to_string()),
                ExprInstance::GInt(i) => Ok(i.to_string()),
                ExprInstance::GString(s) => Ok(format!("\"{}\"", s)),
                ExprInstance::GUri(u) => Ok(format!("`{}`", u)),
                ExprInstance::EMethodBody(method) => {
                    let args: Vec<String> = method
                        .arguments
                        .iter()
                        .map(|arg| self.oracle_build_string_from_message(arg))
                        .collect();

                    let args_string = args.join(", ");

                    Ok(format!(
                        "({}).{}({})",
                        self.oracle_build_string_from_message(
                            method
                                .target
                                .as_ref()
                                .expect("target field on Method was None, should be Some")
                        ),
                        method.method_name,
                        args_string
                    ))
                }
                ExprInstance::GByteArray(bs) => Ok(hex::encode(bs)),
                ExprInstance::GDouble(bits) => {
                    let f = f64::from_bits(*bits);
                    if f == f.floor() && f.is_finite() {
                        Ok(format!("{:.1}f64", f))
                    } else {
                        Ok(format!("{}f64", f))
                    }
                }
                ExprInstance::GBigInt(bytes) => {
                    Ok(format!("{}n", twos_complement_to_decimal(bytes)))
                }
                ExprInstance::GBigRat(rat) => {
                    let num_str = twos_complement_to_decimal(&rat.numerator);
                    let den_str = twos_complement_to_decimal(&rat.denominator);
                    Ok(format!("{}/{}r", num_str, den_str))
                }
                ExprInstance::GFixedPoint(fp) => {
                    let unscaled_str = twos_complement_to_decimal(&fp.unscaled);
                    if fp.scale == 0 {
                        Ok(format!("{}p0", unscaled_str))
                    } else {
                        let scale = fp.scale as usize;
                        let is_negative = unscaled_str.starts_with('-');
                        let digits = if is_negative {
                            &unscaled_str[1..]
                        } else {
                            &unscaled_str
                        };
                        if digits.len() <= scale {
                            let padded = format!("{:0>width$}", digits, width = scale + 1);
                            let (integer, fraction) = padded.split_at(padded.len() - scale);
                            let prefix = if is_negative { "-" } else { "" };
                            Ok(format!("{}{}.{}p{}", prefix, integer, fraction, scale))
                        } else {
                            let (integer, fraction) = digits.split_at(digits.len() - scale);
                            let prefix = if is_negative { "-" } else { "" };
                            Ok(format!("{}{}.{}p{}", prefix, integer, fraction, scale))
                        }
                    }
                }
            },
            // TODO: Figure out if we can prevent prost from generating - OLD
            None => Ok(String::from("Nil")),
        }
    }

    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 732-789, at commit 739368a4
    // ===================================================================
    fn _oracle_build_channel_string(
        &mut self,
        p: &Par,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        let quote_if_not_new = |s: String, news_shift_indices: &[NewBindRange], bound_shift: i32| {
            let is_bound_new = match p.exprs.as_slice() {
                [x] => match &x.expr_instance {
                    Some(instance) => match instance {
                        ExprInstance::EVarBody(EVar { v }) => match v {
                            Some(v) => match &v.var_instance {
                                Some(instance) => match instance {
                                    VarInstance::BoundVar(level) => PrettyPrinter::is_new_var(
                                        level,
                                        news_shift_indices,
                                        bound_shift,
                                    ),
                                    _ => false,
                                },
                                None => false,
                            },
                            None => false,
                        },

                        _ => false,
                    },
                    None => false,
                },
                _ => false,
            };

            if is_bound_new {
                s
            } else {
                format!("@{{{}}}", s)
            }
        };

        self.is_building_channel = true;
        let str = self._oracle_build_string_from_message(PpNode::Par(p), indent)?;
        if str.len() > 60 {
            Ok(quote_if_not_new(
                str,
                &self.news_shift_indices,
                self.bound_shift,
            ))
        } else {
            let whitespace = "\n(\\s\\s)*";
            let replaced = regex::Regex::new(whitespace)
                .unwrap()
                .replace_all(&str, " ");
            Ok(quote_if_not_new(
                replaced.to_string(),
                &self.news_shift_indices,
                self.bound_shift,
            ))
        }
    }

    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 791-1151, at commit 739368a4
    // ===================================================================
    fn _oracle_build_string_from_message(
        &mut self,
        node: PpNode<'_>,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        match node {
        PpNode::Var(v) => {
            Ok(self.build_string_from_var(v))
        }
        PpNode::Unprintable(msg) => Err(InterpreterError::BugFoundError(msg.to_string())),
        PpNode::Send(s) => {
            let str = if s.persistent {
                String::from("!!(")
            } else {
                String::from("!(")
            };

            let data_str = s
                .data
                .iter()
                .map(|p| self.oracle_build_string_from_message(p))
                .collect::<Vec<String>>()
                .join(", ");

            Ok(format!(
                "{}{}{})",
                self.oracle_build_string_from_message(
                    s.chan
                        .as_ref()
                        .expect("channel field on Send was None, should be Some")
                ),
                str,
                data_str
            ))
        }
        PpNode::Receive(r) => {
            let (totally_free, binds_string) = r.binds.iter().enumerate().try_fold(
                (0, String::from("")),
                |(previous_free, mut string), (i, bind)| {
                    self.free_shift = self.bound_shift + previous_free;
                    self.bound_shift = 0;
                    self.free_id = self.bound_id();
                    self.base_id = self.set_base_id();

                    let bind_string = self.oracle_build_pattern(&bind.patterns);
                    string.push_str(&bind_string);

                    if r.persistent {
                        string.push_str(" <= ");
                    } else if r.peek {
                        string.push_str(" <<- ");
                    } else {
                        string.push_str(" <- ");
                    }

                    string.push_str(
                        &self._oracle_build_channel_string(
                            bind.source
                                .as_ref()
                                .expect("source field on bind was None, should be Some"),
                            indent,
                        )?,
                    );

                    if i != r.binds.len() - 1 {
                        string.push_str("  & ");
                    }

                    Ok::<(i32, std::string::String), InterpreterError>((
                        bind.free_count + previous_free,
                        string,
                    ))
                },
            )?;

            self.bound_shift += totally_free;
            let body_str = self.oracle_build_string_from_message(
                r.body
                    .as_ref()
                    .expect("body field on receive was None, should be Some"),
            );

            if !body_str.is_empty() {
                Ok(format!(
                    "for( {} ) {{\n{}{}{}\n{}}}",
                    binds_string,
                    self.indent_string().repeat(indent + 1),
                    body_str,
                    self.indent_string().repeat(indent),
                    ""
                ))
            } else {
                Ok(format!("for( {} ) {{}}", binds_string))
            }
        }
        PpNode::Bundle(b) => {
            Ok(format!(
                "{}{{\n{}{}\n}}",
                BundleOps::show(b),
                self.indent_string().repeat(indent + 1),
                self._oracle_build_string_from_message(
                    PpNode::Par(
                        b.body
                            .as_ref()
                            .expect("body field on bundle was None, should be Some")
                    ),
                    indent + 1
                )?
            ))
        }
        PpNode::New(n) => {
            // ⚠ NOT verbatim, and deliberately so: the ONE edit this body has
            // taken since it was copied. `(0..n.bind_count).map(|i| i +
            // self.bound_shift).collect()` was an unbounded `Vec<i32>`; the
            // contiguous run it built is now held as the interval itself,
            // identically to `descend_node`'s `PpNode::New`. The twin has to
            // move with the driver or the differential compares two different
            // computations.
            let introduced = self.new_bind_range(n.bind_count);

            let result = format!(
                "new {} in {{\n{}{}",
                self.build_variables(introduced),
                self.indent_string().repeat(indent + 1),
                {
                    self.bound_shift += n.bind_count;
                    self.news_shift_indices.push(introduced);
                    self._oracle_build_string_from_message(
                        PpNode::Par(
                            n.p.as_ref()
                                .expect("p field on New was None, should be Some"),
                        ),
                        indent + 1,
                    )?
                }
            );

            Ok(format!(
                "{}\n{}{}",
                result,
                self.indent_string().repeat(indent),
                "}"
            ))
        }
        PpNode::Expr(e) => {
            Ok(self.oracle_build_string_from_expr(e))
        }
        PpNode::Match(m) => {
            let result = format!(
                "match {} {{\n{}{}",
                // ⚠ NOT verbatim, and deliberately so: the ONE other edit this
                // body has taken. The copied line was
                // `self.build_string_from_message(&m.target)`, and `m.target`
                // is an `Option<Par>`, which matched no `downcast_ref` arm, so
                // EVERY `match` term rendered its target as an error string.
                // The closed `PpNode` dispatch turned that into a type error;
                // the repair keeps the SAME entry point the copied line named
                // (`*_build_string_from_message`, catching + capped + indent 0)
                // and projects the `Option` with the same `.expect` discipline
                // every other required `Option<Par>` field in this file uses.
                //
                // Changed identically in `descend_node`'s `PpNode::Match`; see
                // `super::PpNode` for the byte delta and why it needed saying.
                self.oracle_build_string_from_message(
                    m.target
                        .as_ref()
                        .expect("target field on Match was None, should be Some"),
                ),
                self.indent_string().repeat(indent + 1),
                m.cases.iter().enumerate().fold(
                    Ok(String::new()),
                    |acc: Result<String, InterpreterError>, (i, match_case)| {
                        let string = acc?;

                        let case_string = format!(
                            "{}{}{}",
                            self.indent_string().repeat(indent + 1),
                            self.oracle_build_match_case(match_case, indent + 1)?,
                            if i != m.cases.len() - 1 { "\n" } else { "" }
                        );

                        Ok(string + &case_string)
                    }
                )?
            );

            Ok(format!(
                "{}\n{}{}",
                result,
                self.indent_string().repeat(indent),
                "}"
            ))
        }
        PpNode::Unforgeable(u) => {
            self.build_string_from_unforgeable(u)
        }
        PpNode::Connective(c) => {
            match &c.connective_instance {
                Some(conn_instance) => match conn_instance {
                    ConnectiveInstance::ConnAndBody(value) => Ok(format!(
                        "{{ {} }}",
                        value
                            .ps
                            .iter()
                            .map(|p| self.oracle_build_string_from_message(p))
                            .collect::<Vec<String>>()
                            .join(" /\\ ")
                    )),
                    ConnectiveInstance::ConnOrBody(value) => Ok(format!(
                        "{{ {} }}",
                        value
                            .ps
                            .iter()
                            .map(|p| self.oracle_build_string_from_message(p))
                            .collect::<Vec<String>>()
                            .join(" \\/ ")
                    )),
                    ConnectiveInstance::ConnNotBody(value) => {
                        Ok(format!("~{{{}}}", self.oracle_build_string_from_message(value)))
                    }
                    ConnectiveInstance::VarRefBody(value) => Ok(format!(
                        "={}{}",
                        self.free_id,
                        self.free_shift - value.index - 1
                    )),
                    ConnectiveInstance::ConnBool(_) => Ok(String::from("Bool")),
                    ConnectiveInstance::ConnInt(_) => Ok(String::from("Int")),
                    ConnectiveInstance::ConnString(_) => Ok(String::from("String")),
                    ConnectiveInstance::ConnUri(_) => Ok(String::from("Uri")),
                    ConnectiveInstance::ConnByteArray(_) => Ok(String::from("ByteArray")),
                },
                None => Ok(String::new()),
            }
        }
        PpNode::Par(p) => {
            if self.is_empty_par(p) {
                Ok(String::from("Nil"))
            } else {
                // Iterate through Par fields directly (like Scala does) instead of boxing and downcasting
                // This avoids type erasure issues that cause panics when downcast_ref fails
                let mut prev_non_empty = false;
                let mut result = String::new();

                // Process bundles
                if !p.bundles.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, bundle) in p.bundles.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Bundle(bundle), indent)?);
                        if index != p.bundles.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process sends
                if !p.sends.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, send) in p.sends.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Send(send), indent)?);
                        if index != p.sends.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process receives
                if !p.receives.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, receive) in p.receives.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Receive(receive), indent)?);
                        if index != p.receives.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process news
                if !p.news.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, new_item) in p.news.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::New(new_item), indent)?);
                        if index != p.news.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process exprs
                if !p.exprs.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, expr) in p.exprs.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Expr(expr), indent)?);
                        if index != p.exprs.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process matches
                if !p.matches.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, match_item) in p.matches.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Match(match_item), indent)?);
                        if index != p.matches.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process unforgeables
                if !p.unforgeables.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, unforgeable) in p.unforgeables.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Unforgeable(unforgeable), indent)?);
                        if index != p.unforgeables.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                    prev_non_empty = true;
                }

                // Process connectives
                if !p.connectives.is_empty() {
                    if prev_non_empty {
                        result.push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                    }
                    for (index, connective) in p.connectives.iter().enumerate() {
                        result.push_str(&self._oracle_build_string_from_message(PpNode::Connective(connective), indent)?);
                        if index != p.connectives.len() - 1 {
                            result
                                .push_str(&format!(" |\n{}", self.indent_string().repeat(indent)));
                        }
                    }
                }

                Ok(result)
            }
        }
        }
    }

    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 1182-1194, at commit 739368a4
    // ===================================================================
    fn oracle_build_vec(&mut self, s: &Vec<Par>) -> String {
        s.iter().enumerate().fold(String::new(), |string, (i, p)| {
            let mut result = string;

            result.push_str(&self.oracle_build_string_from_message(p));

            if i != s.len() - 1 {
                result.push_str(", ");
            }

            result
        })
    }

    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 1196-1211, at commit 739368a4
    // ===================================================================
    fn oracle_build_pattern(&mut self, patterns: &Vec<Par>) -> String {
        patterns
            .iter()
            .enumerate()
            .fold(String::new(), |string, (i, pattern)| {
                let mut result = string;

                result.push_str(&self.oracle_build_channel_string(pattern));

                if i != patterns.len() - 1 {
                    result.push_str(", ");
                }

                result
            })
    }

    // ===================================================================
    // VERBATIM from rholang/src/rust/interpreter/pretty_printer.rs, lines 1213-1253, at commit 739368a4
    // ===================================================================
    fn oracle_build_match_case(
        &mut self,
        match_case: &MatchCase,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        let pattern_free = match_case.free_count;
        let open_brace = format!("{{\n{}", self.indent_string().repeat(indent + 1));
        let close_brace = format!("\n{}}}", self.indent_string().repeat(indent));

        self.free_shift = self.bound_shift;
        self.bound_shift = 0;
        self.free_id = self.bound_id();
        self.base_id = self.set_base_id();

        Ok(format!(
            "{} => {}{}{}",
            self._oracle_build_string_from_message(
                PpNode::Par(
                    match_case
                        .pattern
                        .as_ref()
                        .expect("pattern field on MatchCase was None, should be Some")
                ),
                indent
            )?,
            open_brace,
            {
                self.bound_shift += pattern_free;
                self._oracle_build_string_from_message(
                    PpNode::Par(
                        match_case
                            .source
                            .as_ref()
                            .expect("source field on MatchCase was None, should be Some"),
                    ),
                    indent + 1,
                )?
            },
            close_brace
        ))
    }
}
