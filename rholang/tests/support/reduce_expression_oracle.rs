// Test-only recursive evaluator oracle, physically separated from production
// sources and included only when `cfg(test)` is active in `reduce.rs`.
macro_rules! reducer_expression_oracle_methods {
    () => {
        // RECURSIVE TWIN — the oracle for the differential harness (`differential`
        // module). Scalar/operator arms call the SAME shared `combine_*` helpers
        // as the trampoline. Five collection arms independently rebuild ordinary
        // values while production builds `ParConstruction` plans; byte-identical
        // results prove that representation boundary as well as the trampoline's
        // descend/combine wiring. cfg(test): excluded from production.
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
        // because per-arm scalar/operator logic was LIFTED OUT into shared
        // `combine_*` helpers. The collection shells remain independent so the
        // oracle can detect a defect in planned construction.
        //
        // ⚠ THE CONSEQUENCE, stated so nobody has to infer it: this oracle SHARES
        // scalar/operator code with the machine it checks. It CANNOT detect a bug
        // inside one of those shared helpers. It CAN detect planned-construction
        // defects for EList, ETuple, EPathMap, ESet, and EMap because these methods
        // independently rebuild decoded values. This is a deliberate, measured
        // boundary, not a claim that the complete 1,213-line arm table is copied.
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
            let expressions = par.decode_occurrences::<Expr>();
            let evaled_exprs = expressions
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
        fn eval_single_step_recursive(
            &self,
            step: EvalSingleStep,
            env: &Env<Par>,
        ) -> Result<Expr, InterpreterError> {
            match step {
                EvalSingleStep::Complete(expr) => Ok(expr),
                EvalSingleStep::Evaluate(par) => self.eval_single_expr_recursive(&par, env),
            }
        }

        #[cfg(test)]
        fn interpolate_step_recursive(
            &self,
            step: InterpolateStep,
            env: &Env<Par>,
        ) -> Result<Expr, InterpreterError> {
            match step {
                InterpolateStep::Complete(expr) => Ok(expr),
                InterpolateStep::Evaluate { source, pairs } => {
                    let pair_count = pairs.len();
                    let mut evaled = Vec::with_capacity(
                        pair_count
                            .checked_mul(2)
                            .expect("an addressable interpolation map cannot overflow"),
                    );
                    for (key, value) in pairs {
                        evaled.push(self.eval_single_expr_recursive(&key, env)?);
                        evaled.push(self.eval_single_expr_recursive(&value, env)?);
                    }
                    self.finish_percent_percent(source, pair_count, evaled)
                }
            }
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
                    let result_par = self.apply_method_recursive(
                        emethod,
                        evaled_target,
                        evaled_args,
                        env,
                        MethodOutput::Par,
                    )?;
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
                        let step = self.combine_plus(v1, v2, env)?;
                        self.eval_single_step_recursive(step, env)
                    }
                    ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        let step = self.combine_minus(v1, v2, env)?;
                        self.eval_single_step_recursive(step, env)
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
                        let step = self.combine_percent_percent(v1, v2)?;
                        self.interpolate_step_recursive(step, env)
                    }
                    ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => {
                        self.metering.reserve_primitive(op_call_cost())?;
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        let step = self.combine_plus_plus(v1, v2, env)?;
                        self.eval_single_step_recursive(step, env)
                    }
                    ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
                        self.metering.reserve_primitive(op_call_cost())?;
                        let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                        let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                        let step = self.combine_minus_minus(v1, v2, env)?;
                        self.eval_single_step_recursive(step, env)
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
                        let evaled = self
                            .prepare_eset(eset)
                            .into_iter()
                            .map(|q| self.eval_expr_recursive(&q, env))
                            .collect::<Result<Vec<_>, InterpreterError>>()?;
                        self.combine_eset(evaled, eset)
                    }
                    ExprInstance::EMapBody(emap) => {
                        let evaled = self
                            .prepare_emap(emap)
                            .into_iter()
                            .map(|(key, value)| {
                                Ok((
                                    self.eval_expr_recursive(&key, env)?,
                                    self.eval_expr_recursive(&value, env)?,
                                ))
                            })
                            .collect::<Result<Vec<_>, InterpreterError>>()?;
                        self.combine_emap(evaled, emap)
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
                        let result_par = self.apply_method_recursive(
                            emethod,
                            evaled_target,
                            evaled_args,
                            env,
                            MethodOutput::Expr,
                        )?;
                        self.eval_single_expr_recursive(&result_par, env)
                    }
                },
                None => Err(InterpreterError::ReduceError(format!(
                    "Unimplemented expression: {:?}",
                    expr
                ))),
            }
        }

        // The five collection combiners are intentionally oracle-only. Production
        // constructs their results through `plan_*` + `ParConstruction`; keeping
        // these ordinary decoded rebuilds independent lets the differential test
        // that representation change instead of sharing its implementation.
        #[cfg(test)]
        fn combine_elist(
            &self,
            evaled_ps: Vec<Par>,
            e1: &EList,
        ) -> Result<Expr, InterpreterError> {
            let updated_ps: Vec<Par> = evaled_ps
                .into_iter()
                .map(|p| self.update_locally_free_par(p))
                .collect();

            Ok(Expr {
                expr_instance: Some(ExprInstance::EListBody(self.update_locally_free_elist(
                    EList {
                        ps: updated_ps,
                        locally_free: e1.locally_free.clone(),
                        connective_used: e1.connective_used,
                        remainder: None,
                    },
                ))),
            })
        }

        #[cfg(test)]
        fn combine_etuple(
            &self,
            evaled_ps: Vec<Par>,
            e1: &ETuple,
        ) -> Result<Expr, InterpreterError> {
            let updated_ps: Vec<Par> = evaled_ps
                .into_iter()
                .map(|p| self.update_locally_free_par(p))
                .collect();

            Ok(Expr {
                expr_instance: Some(ExprInstance::ETupleBody(
                    self.update_locally_free_etuple(ETuple {
                        ps: updated_ps,
                        locally_free: e1.locally_free.clone(),
                        connective_used: e1.connective_used,
                    }),
                )),
            })
        }

        #[cfg(test)]
        fn combine_epathmap(
            &self,
            evaled_ps: Vec<Par>,
            e1: &EPathMap,
        ) -> Result<Expr, InterpreterError> {
            let updated_ps: Vec<Par> = evaled_ps
                .into_iter()
                .map(|p| self.update_locally_free_par(p))
                .collect();

            let rebuilt = match e1.mode() {
                EPathMapMode::Empty | EPathMapMode::Set => EPathMap::new(
                    updated_ps,
                    e1.locally_free.clone(),
                    e1.connective_used,
                    None,
                ),
                EPathMapMode::Map => {
                    let mut values = updated_ps.into_iter();
                    let entries = std::iter::from_fn(|| {
                        let key = values.next()?;
                        let value = values
                            .next()
                            .expect("map-mode evaluator produces one value for every key");
                        Some((key, value))
                    });
                    EPathMap::new_map(
                        entries,
                        e1.locally_free.clone(),
                        e1.connective_used,
                        None,
                    )
                }
            };
            Ok(Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(rebuilt)),
            })
        }

        #[cfg(test)]
        fn combine_eset(
            &self,
            evaled_ps: Vec<Par>,
            eset: &ESet,
        ) -> Result<Expr, InterpreterError> {
            let updated_ps: Vec<Par> = evaled_ps
                .into_iter()
                .map(|p| self.update_locally_free_par(p))
                .collect();
            let rebuilt = ParSet {
                ps: SortedParHashSet::create_from_vec(updated_ps),
                connective_used: eset.connective_used,
                locally_free: eset.locally_free.clone(),
                remainder: eset.remainder.clone(),
            };
            Ok(Expr {
                expr_instance: Some(ExprInstance::ESetBody(
                    ParSetTypeMapper::par_set_to_eset(rebuilt),
                )),
            })
        }

        #[cfg(test)]
        fn combine_emap(
            &self,
            evaled_ps: Vec<(Par, Par)>,
            emap: &EMap,
        ) -> Result<Expr, InterpreterError> {
            let rebuilt = ParMap {
                ps: SortedParMap::create_from_vec(evaled_ps),
                connective_used: emap.connective_used,
                locally_free: emap.locally_free.clone(),
                remainder: emap.remainder.clone(),
            };
            Ok(Expr {
                expr_instance: Some(ExprInstance::EMapBody(
                    ParMapTypeMapper::par_map_to_emap(rebuilt),
                )),
            })
        }

        #[cfg(test)]
        fn eval_single_expr_recursive(
            &self,
            p: &Par,
            env: &Env<Par>,
        ) -> Result<Expr, InterpreterError> {
            if !p.all_components_are::<Expr>() {
                return Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )));
            }
            if p.family_cardinality::<Expr>() != 1 {
                return Err(InterpreterError::ReduceError(
                    "Error: Multiple expressions given.".to_string(),
                ));
            }
            let expression = p
                .first::<Expr>()
                .expect("resident Par Expr endpoints are canonical")
                .expect("Expr cardinality one has exactly one endpoint")
                .0;
            self.eval_expr_to_expr_recursive(&expression, env)
        }

        #[cfg(test)]
        fn eval_to_i64_recursive(&self, p: &Par, env: &Env<Par>) -> Result<i64, InterpreterError> {
            if !p.all_components_are::<Expr>() {
                return Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )));
            }
            if p.family_cardinality::<Expr>() != 1 {
                return Err(InterpreterError::ReduceError(
                    "Error: Integer expected, or unimplemented expression.".to_string(),
                ));
            }
            let expression = p
                .first::<Expr>()
                .expect("resident Par Expr endpoints are canonical")
                .expect("Expr cardinality one has exactly one endpoint")
                .0;
            match &expression {
                Expr {
                    expr_instance: Some(ExprInstance::GInt(v)),
                } => Ok(*v),
                Expr {
                    expr_instance: Some(ExprInstance::EVarBody(EVar { v })),
                } => {
                    let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                    self.eval_to_i64_recursive(&p, env)
                }
                e => {
                    let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                    Self::extract_i64(evaled)
                }
            }
        }

        #[cfg(test)]
        fn eval_to_bool_recursive(
            &self,
            p: &Par,
            env: &Env<Par>,
        ) -> Result<bool, InterpreterError> {
            if !p.all_components_are::<Expr>() {
                return Err(InterpreterError::ReduceError(String::from(
                    "Error: parallel or non expression found where expression expected.",
                )));
            }
            if p.family_cardinality::<Expr>() != 1 {
                return Err(InterpreterError::ReduceError(
                    "Error: Multiple expressions given.".to_string(),
                ));
            }
            let expression = p
                .first::<Expr>()
                .expect("resident Par Expr endpoints are canonical")
                .expect("Expr cardinality one has exactly one endpoint")
                .0;
            match &expression {
                Expr {
                    expr_instance: Some(ExprInstance::GBool(b)),
                } => Ok(*b),
                Expr {
                    expr_instance: Some(ExprInstance::EVarBody(EVar { v })),
                } => {
                    let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                    self.eval_to_bool_recursive(&p, env)
                }
                e => {
                    let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                    Self::extract_bool(evaled)
                }
            }
        }
    };
}
