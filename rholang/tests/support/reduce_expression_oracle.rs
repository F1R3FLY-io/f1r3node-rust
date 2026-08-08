// Test-only recursive evaluator oracle, physically separated from production
// sources and included only when `cfg(test)` is active in `reduce.rs`.
macro_rules! reducer_expression_oracle_methods {
    () => {
        // RECURSIVE TWIN — the oracle for the differential harness (`differential`
        // module). Each function is a faithful recursive dispatcher over the SAME
        // shared `combine_*` helpers the trampoline uses, so a byte-identical
        // result + charge trace between the two proves the trampoline's descend/
        // combine WIRING (the only new logic). cfg(test): excluded from production.
        //
        // ★ PROVENANCE, and what it is NOT.
        //
        // These six functions REPLACE `eval_expr`, `eval_expr_to_par`,
        // `eval_expr_to_expr`, `eval_single_expr`, `eval_to_i64` and `eval_to_bool`
        // as they stood at `a929a2d6^` (the commit before the trampoline). They are
        // NOT copies of them, and the difference is not incidental — measured, with
        // a `_recursive` rename applied:
        //
        //   | pre-trampoline fn  | its lines | twin lines | byte-identical |
        //   |--------------------|-----------|------------|----------------|
        //   | eval_expr          |        20 |         11 | no             |
        //   | eval_expr_to_par   |        59 |         44 | no             |
        //   | eval_expr_to_expr  |     1,213 |        167 | no             |
        //   | eval_single_expr   |        21 |         20 | no             |
        //   | eval_to_i64        |        48 |         28 | no             |
        //   | eval_to_bool       |        48 |         28 | no             |
        //
        // ZERO of six. `eval_expr_to_expr` alone collapses 1,213 lines to 167,
        // because the per-arm arithmetic, comparison and collection logic was
        // LIFTED OUT into the `combine_*` helpers and the twin now calls them —
        // the same helpers the trampoline calls.
        //
        // ⚠ THE CONSEQUENCE, stated so nobody has to infer it: this oracle SHARES
        // code with the machine it checks. The differential can therefore prove the
        // trampoline's descend/combine WIRING — which is the only new logic, and is
        // what the harness claims — and it CANNOT detect a bug inside a `combine_*`
        // helper, because both sides would compute the same wrong answer. That is a
        // deliberate, bounded trade (a duplicated 1,213-line arm table would test
        // the copy rather than the driving), not an oversight; it is written down
        // here because "faithful copy" invited exactly the opposite reading.
        //
        // `rholang/tests/normalize_oracle_provenance.rs::
        // the_trampoline_twin_is_a_rewrite_not_a_copy` re-derives the table above
        // from git and fails if any entry moves.
        //
        // ⚠ NO `rustfmt.toml` exclusion, and that is the point: this file asserts no
        // byte-identity, so formatting it falsifies nothing. An exclusion here would
        // freeze text whose claim is "shares the combiners", which no formatter can
        // break — see `2fee95d8` for why the criterion is stated that narrowly.
        // =======================================================================
        #[cfg(test)]
        pub(crate) fn eval_expr_recursive(
            &self,
            par: &Par,
            env: &Env<Par>,
        ) -> Result<Par, InterpreterError> {
            let evaled_exprs = par
                .exprs
                .iter()
                .map(|expr| self.eval_expr_to_par_recursive(expr, env))
                .collect::<Result<Vec<_>, InterpreterError>>()?;
            let result = evaled_exprs
                .into_iter()
                .fold(par.with_exprs(Vec::new()), |acc, expr| {
                    concatenate_pars(acc, expr)
                });
            Ok(result)
        }

        #[cfg(test)]
        fn eval_expr_to_par_recursive(
            &self,
            expr: &Expr,
            env: &Env<Par>,
        ) -> Result<Par, InterpreterError> {
            if let Some(ExprInstance::EMethodBody(emethod)) = &expr.expr_instance {
                if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                    return Ok(fused);
                }
            }
            let expr_instance = match &expr.expr_instance {
                Some(ei) => ei,
                None => {
                    return Err(InterpreterError::UndefinedRequiredProtobufFieldError(
                        format!("{:?}", std::any::type_name::<ExprInstance>()),
                    ))
                }
            };
            match expr_instance {
                ExprInstance::EVarBody(evar) => {
                    let p = self.eval_var(&unwrap_option_safe(evar.v.clone())?, env)?;
                    let evaled_p = self.eval_expr_recursive(&p, env)?;
                    Ok(evaled_p)
                }
                ExprInstance::EMethodBody(emethod) => {
                    self.metering.reserve_primitive(method_call_cost())?;
                    let evaled_target = self
                        .eval_expr_recursive(&unwrap_option_safe(emethod.target.clone())?, env)?;
                    let evaled_args: Vec<Par> = emethod
                        .arguments
                        .iter()
                        .map(|arg| self.eval_expr_recursive(arg, env))
                        .collect::<Result<Vec<_>, InterpreterError>>()?;
                    let result_par = match self.method_table().get(&emethod.method_name) {
                        Some(_method) => _method.apply(evaled_target, evaled_args, env)?,
                        None => {
                            return Err(InterpreterError::ReduceError(format!(
                                "Unimplemented method: {}",
                                emethod.method_name
                            )));
                        }
                    };
                    Ok(result_par)
                }
                _ => {
                    Ok(Par::default()
                        .with_exprs(vec![self.eval_expr_to_expr_recursive(expr, env)?]))
                }
            }
        }

        #[cfg(test)]
        fn eval_expr_to_expr_recursive(
            &self,
            expr: &Expr,
            env: &Env<Par>,
        ) -> Result<Expr, InterpreterError> {
            match &expr.expr_instance {
                Some(expr_instance) => match expr_instance {
                    ExprInstance::GBool(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GBool(*x)),
                    }),
                    ExprInstance::GInt(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GInt(*x)),
                    }),
                    ExprInstance::GString(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GString(x.clone())),
                    }),
                    ExprInstance::GUri(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GUri(x.clone())),
                    }),
                    ExprInstance::GByteArray(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GByteArray(x.clone())),
                    }),
                    ExprInstance::GDouble(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GDouble(*x)),
                    }),
                    ExprInstance::GBigInt(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GBigInt(x.clone())),
                    }),
                    ExprInstance::GBigRat(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GBigRat(x.clone())),
                    }),
                    ExprInstance::GFixedPoint(x) => Ok(Expr {
                        expr_instance: Some(ExprInstance::GFixedPoint(x.clone())),
                    }),
                    ExprInstance::EZipperBody(zipper) => Ok(Expr {
                        expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())),
                    }),

                    ExprInstance::ENotBody(enot) => {
                        let b = self.eval_to_bool_recursive(enot.p.as_ref().unwrap(), env)?;
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::GBool(!b)),
                        })
                    }
                    ExprInstance::ENegBody(eneg) => {
                        let v = self.eval_single_expr_recursive(eneg.p.as_ref().unwrap(), env)?;
                        self.combine_neg(v)
                    }
                    ExprInstance::EMultBody(EMult { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_mult(v1, v2)
                    }
                    ExprInstance::EDivBody(EDiv { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_div(v1, v2)
                    }
                    ExprInstance::EModBody(EMod { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_mod(v1, v2)
                    }
                    ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_plus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                    }
                    ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_minus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                    }
                    ExprInstance::ELtBody(ELt { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_relop(
                            v1,
                            v2,
                            |b1, b2| !b1 & b2,
                            |i1, i2| i1 < i2,
                            |s1, s2| s1 < s2,
                        )
                    }
                    ExprInstance::ELteBody(ELte { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_relop(
                            v1,
                            v2,
                            |b1, b2| b1 <= b2,
                            |i1, i2| i1 <= i2,
                            |s1, s2| s1 <= s2,
                        )
                    }
                    ExprInstance::EGtBody(EGt { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_relop(
                            v1,
                            v2,
                            |b1, b2| b1 & !b2,
                            |i1, i2| i1 > i2,
                            |s1, s2| s1 > s2,
                        )
                    }
                    ExprInstance::EGteBody(EGte { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_relop(
                            v1,
                            v2,
                            |b1, b2| b1 >= b2,
                            |i1, i2| i1 >= i2,
                            |s1, s2| s1 >= s2,
                        )
                    }
                    ExprInstance::EEqBody(EEq { p1, p2 }) => {
                        let v1 = self.eval_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_eq(v1, v2, env)
                    }
                    ExprInstance::ENeqBody(ENeq { p1, p2 }) => {
                        let v1 = self.eval_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_neq(v1, v2, env)
                    }
                    ExprInstance::EAndBody(EAnd { p1, p2 }) => {
                        let b1 = self.eval_to_bool_recursive(p1.as_ref().unwrap(), env)?;
                        let b2 = self.eval_to_bool_recursive(p2.as_ref().unwrap(), env)?;
                        self.metering.reserve_primitive(boolean_and_cost())?;
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::GBool(b1 && b2)),
                        })
                    }
                    ExprInstance::EOrBody(EOr { p1, p2 }) => {
                        let b1 = self.eval_to_bool_recursive(p1.as_ref().unwrap(), env)?;
                        let b2 = self.eval_to_bool_recursive(p2.as_ref().unwrap(), env)?;
                        self.metering.reserve_primitive(boolean_or_cost())?;
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::GBool(b1 || b2)),
                        })
                    }
                    ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
                        let evaled_target =
                            self.eval_expr_recursive(target.as_ref().unwrap(), env)?;
                        self.combine_matches(evaled_target, pattern.as_ref().unwrap(), env)
                    }
                    ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => {
                        self.metering.reserve_primitive(op_call_cost())?;
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_percent_percent(v1, v2, |q| {
                            self.eval_single_expr_recursive(q, env)
                        })
                    }
                    ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => {
                        self.metering.reserve_primitive(op_call_cost())?;
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_plus_plus(v1, v2, env, |q| {
                            self.eval_single_expr_recursive(q, env)
                        })
                    }
                    ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
                        self.metering.reserve_primitive(op_call_cost())?;
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        self.combine_minus_minus(v1, v2, env, |q| {
                            self.eval_single_expr_recursive(q, env)
                        })
                    }
                    ExprInstance::EVarBody(EVar { v }) => {
                        let p = self.eval_var(v.as_ref().unwrap(), env)?;
                        self.eval_single_expr_recursive(&p, env)
                    }
                    ExprInstance::EListBody(e1) => {
                        let evaled_ps = e1
                            .ps
                            .iter()
                            .map(|p| self.eval_expr_recursive(p, env))
                            .collect::<Result<Vec<_>, InterpreterError>>()?;
                        self.combine_elist(evaled_ps, e1)
                    }
                    ExprInstance::ETupleBody(e1) => {
                        let evaled_ps = e1
                            .ps
                            .iter()
                            .map(|p| self.eval_expr_recursive(p, env))
                            .collect::<Result<Vec<_>, InterpreterError>>()?;
                        self.combine_etuple(evaled_ps, e1)
                    }
                    ExprInstance::EPathmapBody(e1) => {
                        // ★ Walks the TRIE. `ps()` forces `EntryTrie::view`, which
                        // deep-clones every entry, to build a `Vec` this only iterates once
                        // — and the evaluation can fail, so the borrowing walk has to carry
                        // the `?`. Preallocated from the O(1) maintained fold.
                        let mut evaled_ps = Vec::with_capacity(e1.entry_trie().len());
                        e1.entry_trie().try_for_each_entry(
                            |p| -> Result<(), InterpreterError> {
                                evaled_ps.push(self.eval_expr_recursive(p, env)?);
                                Ok(())
                            },
                        )?;
                        self.combine_epathmap(evaled_ps, e1)
                    }
                    ExprInstance::ESetBody(eset) => {
                        self.combine_eset(eset, |q| self.eval_expr_recursive(q, env))
                    }
                    ExprInstance::EMapBody(emap) => {
                        self.combine_emap(emap, |q| self.eval_expr_recursive(q, env))
                    }
                    ExprInstance::EMethodBody(emethod) => {
                        if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                            return self.eval_single_expr_recursive(&fused, env);
                        }
                        self.metering.reserve_primitive(method_call_cost())?;
                        let evaled_target =
                            self.eval_expr_recursive(emethod.target.as_ref().unwrap(), env)?;
                        let evaled_args: Vec<Par> = emethod
                            .arguments
                            .iter()
                            .map(|arg| self.eval_expr_recursive(arg, env))
                            .collect::<Result<Vec<_>, InterpreterError>>()?;
                        let result_par =
                            self.apply_method_expr(emethod, evaled_target, evaled_args, env)?;
                        self.eval_single_expr_recursive(&result_par, env)
                    }
                },
                None => Err(InterpreterError::ReduceError(format!(
                    "Unimplemented expression: {:?}",
                    expr
                ))),
            }
        }

        #[cfg(test)]
        fn eval_single_expr_recursive(
            &self,
            p: &Par,
            env: &Env<Par>,
        ) -> Result<Expr, InterpreterError> {
            if !p.sends.is_empty()
                || !p.receives.is_empty()
                || !p.news.is_empty()
                || !p.matches.is_empty()
                || !p.unforgeables.is_empty()
                || !p.bundles.is_empty()
            {
                Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )))
            } else {
                match p.exprs.as_slice() {
                    [e] => Ok(self.eval_expr_to_expr_recursive(e, env)?),
                    _ => Err(InterpreterError::ReduceError(
                        "Error: Multiple expressions given.".to_string(),
                    )),
                }
            }
        }

        #[cfg(test)]
        fn eval_to_i64_recursive(&self, p: &Par, env: &Env<Par>) -> Result<i64, InterpreterError> {
            if !p.sends.is_empty()
                && !p.receives.is_empty()
                && !p.news.is_empty()
                && !p.matches.is_empty()
                && !p.unforgeables.is_empty()
                && !p.bundles.is_empty()
            {
                Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )))
            } else {
                match p.exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::GInt(v)),
                    }] => Ok(*v),
                    [Expr {
                        expr_instance: Some(ExprInstance::EVarBody(EVar { v })),
                    }] => {
                        let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                        self.eval_to_i64_recursive(&p, env)
                    }
                    [e] => {
                        let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                        Self::extract_i64(evaled)
                    }
                    _ => Err(InterpreterError::ReduceError(
                        "Error: Integer expected, or unimplemented expression.".to_string(),
                    )),
                }
            }
        }

        #[cfg(test)]
        fn eval_to_bool_recursive(
            &self,
            p: &Par,
            env: &Env<Par>,
        ) -> Result<bool, InterpreterError> {
            if !p.sends.is_empty()
                && !p.receives.is_empty()
                && !p.news.is_empty()
                && !p.matches.is_empty()
                && !p.unforgeables.is_empty()
                && !p.bundles.is_empty()
            {
                Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )))
            } else {
                match p.exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::GBool(b)),
                    }] => Ok(*b),
                    [Expr {
                        expr_instance: Some(ExprInstance::EVarBody(EVar { v })),
                    }] => {
                        let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                        self.eval_to_bool_recursive(&p, env)
                    }
                    [e] => {
                        let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                        Self::extract_bool(evaled)
                    }
                    _ => Err(InterpreterError::ReduceError(
                        "Error: Multiple expressions given.".to_string(),
                    )),
                }
            }
        }
    };
}
