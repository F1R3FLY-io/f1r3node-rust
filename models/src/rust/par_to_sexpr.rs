// Converts Rholang Par AST to S-expression string representation
// This is used to generate deterministic path keys for PathMap

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{Bundle, Expr, New, Par, Receive, Send};

pub struct ParToSExpr;

impl ParToSExpr {
    /// Convert a Par to an S-expression string
    pub fn par_to_sexpr(par: &Par) -> String {
        let mut parts = Vec::new();

        // Process sends
        for send in &par.sends {
            parts.push(Self::send_to_sexpr(send));
        }

        // Process receives
        for receive in &par.receives {
            parts.push(Self::receive_to_sexpr(receive));
        }

        // Process news
        for new in &par.news {
            parts.push(Self::new_to_sexpr(new));
        }

        // Process expressions
        for expr in &par.exprs {
            parts.push(Self::expr_to_sexpr(expr));
        }

        // Process bundles
        for bundle in &par.bundles {
            parts.push(Self::bundle_to_sexpr(bundle));
        }

        // If multiple parts, wrap in a sequence
        if parts.is_empty() {
            String::from("Nil")
        } else if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!("(par {})", parts.join(" "))
        }
    }

    fn send_to_sexpr(send: &Send) -> String {
        let chan = send
            .chan
            .as_ref()
            .map(|c| Self::par_to_sexpr(c))
            .unwrap_or_else(|| "Nil".to_string());
        let data: Vec<String> = send.data.iter().map(|d| Self::par_to_sexpr(d)).collect();
        format!("(! {} {})", chan, data.join(" "))
    }

    fn receive_to_sexpr(receive: &Receive) -> String {
        let binds: Vec<String> = receive
            .binds
            .iter()
            .map(|b| {
                let source = b
                    .source
                    .as_ref()
                    .map(|s| Self::par_to_sexpr(s))
                    .unwrap_or_else(|| "Nil".to_string());
                format!("(bind <- {})", source)
            })
            .collect();
        let body = receive
            .body
            .as_ref()
            .map(|b| Self::par_to_sexpr(b))
            .unwrap_or_else(|| "Nil".to_string());
        format!("(for ({}) {})", binds.join(" "), body)
    }

    fn new_to_sexpr(new: &New) -> String {
        let vars: Vec<String> = (0..new.bind_count).map(|i| format!("x{}", i)).collect();
        let body = new
            .p
            .as_ref()
            .map(|p| Self::par_to_sexpr(p))
            .unwrap_or_else(|| "Nil".to_string());
        format!("(new {} {})", vars.join(" "), body)
    }

    fn bundle_to_sexpr(bundle: &Bundle) -> String {
        let body = bundle
            .body
            .as_ref()
            .map(|b| Self::par_to_sexpr(b))
            .unwrap_or_else(|| "Nil".to_string());
        format!("(bundle {})", body)
    }

    fn expr_to_sexpr(expr: &Expr) -> String {
        match &expr.expr_instance {
            Some(expr_instance) => match expr_instance {
                ExprInstance::GBool(b) => format!("{}", b),
                ExprInstance::GInt(i) => format!("{}", i),
                ExprInstance::GString(s) => format!("\"{}\"", s),
                ExprInstance::GUri(u) => format!("`{}`", u),
                ExprInstance::GByteArray(ba) => format!("0x{}", hex::encode(ba)),

                ExprInstance::EListBody(list) => {
                    let elements: Vec<String> =
                        list.ps.iter().map(|p| Self::par_to_sexpr(p)).collect();
                    format!("[{}]", elements.join(" "))
                }

                ExprInstance::ETupleBody(tuple) => {
                    let elements: Vec<String> =
                        tuple.ps.iter().map(|p| Self::par_to_sexpr(p)).collect();
                    format!("(tuple {})", elements.join(" "))
                }

                ExprInstance::ESetBody(set) => {
                    let elements: Vec<String> =
                        set.ps.iter().map(|p| Self::par_to_sexpr(p)).collect();
                    format!("(set {})", elements.join(" "))
                }

                ExprInstance::EMapBody(map) => {
                    let pairs: Vec<String> = map
                        .kvs
                        .iter()
                        .map(|kv| {
                            format!(
                                "({} : {})",
                                Self::par_to_sexpr(kv.key.as_ref().unwrap_or(&Par::default())),
                                Self::par_to_sexpr(kv.value.as_ref().unwrap_or(&Par::default()))
                            )
                        })
                        .collect();
                    format!("(map {})", pairs.join(" "))
                }

                ExprInstance::EVarBody(evar) => {
                    if let Some(var) = &evar.v {
                        match &var.var_instance {
                            Some(VarInstance::BoundVar(bv)) => format!("_{}", bv),
                            Some(VarInstance::FreeVar(fv)) => format!("${}", fv),
                            Some(VarInstance::Wildcard(_)) => String::from("_"),
                            None => String::from("var"),
                        }
                    } else {
                        String::from("var")
                    }
                }

                ExprInstance::ENegBody(eneg) => {
                    format!(
                        "(- {})",
                        Self::par_to_sexpr(eneg.p.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::ENotBody(enot) => {
                    format!(
                        "(not {})",
                        Self::par_to_sexpr(enot.p.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::EMultBody(emult) => {
                    format!(
                        "(* {} {})",
                        Self::par_to_sexpr(emult.p1.as_ref().unwrap_or(&Par::default())),
                        Self::par_to_sexpr(emult.p2.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::EDivBody(ediv) => {
                    format!(
                        "(/ {} {})",
                        Self::par_to_sexpr(ediv.p1.as_ref().unwrap_or(&Par::default())),
                        Self::par_to_sexpr(ediv.p2.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::EPlusBody(eplus) => {
                    format!(
                        "(+ {} {})",
                        Self::par_to_sexpr(eplus.p1.as_ref().unwrap_or(&Par::default())),
                        Self::par_to_sexpr(eplus.p2.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::EMinusBody(eminus) => {
                    format!(
                        "(- {} {})",
                        Self::par_to_sexpr(eminus.p1.as_ref().unwrap_or(&Par::default())),
                        Self::par_to_sexpr(eminus.p2.as_ref().unwrap_or(&Par::default()))
                    )
                }

                ExprInstance::EMethodBody(method) => {
                    let target =
                        Self::par_to_sexpr(method.target.as_ref().unwrap_or(&Par::default()));
                    let args: Vec<String> = method
                        .arguments
                        .iter()
                        .map(|a| Self::par_to_sexpr(a))
                        .collect();
                    format!("({}.{} {})", target, method.method_name, args.join(" "))
                }

                _ => String::from("(expr)"),
            },
            None => String::from("Nil"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_int() {
        let par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(42)),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "42");
    }

    #[test]
    fn test_simple_string() {
        let par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString("hello".to_string())),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "\"hello\"");
    }

    #[test]
    fn test_list() {
        let par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(crate::rhoapi::EList {
                    ps: vec![
                        Par {
                            exprs: vec![Expr {
                                expr_instance: Some(ExprInstance::GString("a".to_string())),
                            }],
                            ..Default::default()
                        },
                        Par {
                            exprs: vec![Expr {
                                expr_instance: Some(ExprInstance::GString("b".to_string())),
                            }],
                            ..Default::default()
                        },
                    ],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "[\"a\" \"b\"]");
    }

    use crate::rhoapi::{EMethod, KeyValuePair, ReceiveBind};
    use crate::rust::utils::{
        new_boundvar_par, new_bundle_par, new_ediv_expr_gint, new_eminus_expr_gint,
        new_emult_expr_gint, new_eplus_expr_gint, new_freevar_par, new_gbool_par, new_gint_par,
        new_gstring_par, new_guri_par, new_wildcard_par,
    };

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn expr_par(instance: ExprInstance) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn empty_par_renders_as_nil() {
        assert_eq!(ParToSExpr::par_to_sexpr(&Par::default()), "Nil");
    }

    #[test]
    fn ground_values_render_literally() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&new_gbool_par(true, Vec::new(), false)),
            "true"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&new_guri_par(
                "rho:io:stdout".to_string(),
                Vec::new(),
                false
            )),
            "`rho:io:stdout`"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::GByteArray(vec![0xab, 0xcd]))),
            "0xabcd"
        );
    }

    #[test]
    fn vars_render_with_kind_specific_sigils() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&new_boundvar_par(3, Vec::new(), false)),
            "_3"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&new_freevar_par(2, Vec::new())),
            "$2"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&new_wildcard_par(Vec::new(), false)),
            "_"
        );
    }

    #[test]
    fn tuple_set_and_map_render_with_tags() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::ETupleBody(crate::rhoapi::ETuple {
                ps: vec![gint(1), gint(2)],
                locally_free: vec![],
                connective_used: false,
            }))),
            "(tuple 1 2)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::ESetBody(crate::rhoapi::ESet {
                ps: vec![gint(1)],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }))),
            "(set 1)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::EMapBody(crate::rhoapi::EMap {
                kvs: vec![KeyValuePair {
                    key: Some(gint(1)),
                    value: Some(gint(10)),
                }],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }))),
            "(map (1 : 10))"
        );
    }

    #[test]
    fn unary_and_binary_operators_render_prefix() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::ENegBody(crate::rhoapi::ENeg {
                p: Some(gint(4)),
            }))),
            "(- 4)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::ENotBody(crate::rhoapi::ENot {
                p: Some(new_gbool_par(false, Vec::new(), false)),
            }))),
            "(not false)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&Par::default().with_exprs(vec![new_eplus_expr_gint(
                1,
                2,
                Vec::new(),
                false
            )])),
            "(+ 1 2)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&Par::default().with_exprs(vec![new_eminus_expr_gint(
                3,
                1,
                Vec::new(),
                false
            )])),
            "(- 3 1)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&Par::default().with_exprs(vec![new_emult_expr_gint(
                2,
                3,
                Vec::new(),
                false
            )])),
            "(* 2 3)"
        );
        assert_eq!(
            ParToSExpr::par_to_sexpr(&Par::default().with_exprs(vec![new_ediv_expr_gint(
                6,
                2,
                Vec::new(),
                false
            )])),
            "(/ 6 2)"
        );
    }

    #[test]
    fn method_call_renders_target_name_and_args() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(gint(9)),
                arguments: vec![gint(0)],
                locally_free: vec![],
                connective_used: false,
            }))),
            "(9.nth 0)"
        );
    }

    #[test]
    fn send_renders_channel_and_data() {
        let par = Par {
            sends: vec![Send {
                chan: Some(new_gstring_par("chan".to_string(), Vec::new(), false)),
                data: vec![gint(1), gint(2)],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "(! \"chan\" 1 2)");
    }

    #[test]
    fn receive_renders_binds_and_body() {
        let par = Par {
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![],
                    source: Some(new_gstring_par("src".to_string(), Vec::new(), false)),
                    remainder: None,
                    free_count: 0,
                    cost_signature: None,
                }],
                body: Some(gint(1)),
                persistent: false,
                peek: false,
                bind_count: 1,
                locally_free: vec![],
                connective_used: false,
                condition: None,
            }],
            ..Default::default()
        };
        assert_eq!(
            ParToSExpr::par_to_sexpr(&par),
            "(for ((bind <- \"src\")) 1)"
        );
    }

    #[test]
    fn new_renders_generated_variables_and_body() {
        let par = Par {
            news: vec![New {
                bind_count: 2,
                p: Some(gint(7)),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "(new x0 x1 7)");
    }

    #[test]
    fn bundle_renders_wrapped_body() {
        let par = new_bundle_par(gint(5), true, false);
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "(bundle 5)");
    }

    #[test]
    fn multiple_processes_render_as_par_sequence() {
        let par = new_bundle_par(gint(5), true, false).with_exprs(gint(1).exprs);
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "(par 1 (bundle 5))");
    }

    #[test]
    fn unknown_expr_and_missing_instance_have_fallbacks() {
        assert_eq!(
            ParToSExpr::par_to_sexpr(&expr_par(ExprInstance::EPathmapBody(
                crate::rhoapi::EPathMap {
                    ps: vec![],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                }
            ))),
            "(expr)"
        );
        let none_expr = Par {
            exprs: vec![Expr {
                expr_instance: None,
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&none_expr), "Nil");
    }
}
