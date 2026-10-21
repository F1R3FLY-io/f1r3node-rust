use models::rhoapi::Expr;
use models::rust::utils::{
    new_gbigint_expr, new_gbigrat_expr, new_gbool_expr, new_gdouble_expr, new_gfixedpoint_expr,
    new_gfloat32_expr, new_gint32_expr, new_gint_expr, new_gstring_expr, new_guint16_expr,
    new_guint32_expr, new_guint64_expr, new_guint8_expr, new_guri_expr,
};
use rholang_parser::ast::Proc as NewProc;

use crate::rust::interpreter::errors::InterpreterError;

pub fn normalize_ground<'ast>(proc: &NewProc<'ast>) -> Result<Expr, InterpreterError> {
    match proc {
        NewProc::BoolLiteral(value) => Ok(new_gbool_expr(*value)),

        NewProc::LongLiteral(value) => Ok(new_gint_expr(*value)),

        NewProc::SignedIntLiteral { value, bits } => match bits {
            64 => Ok(new_gint_expr(parse_int_literal(value, 'i', *bits)?)),
            32 => Ok(new_gint32_expr(parse_int_literal(value, 'i', *bits)?)),
            _ => Err(unsupported_int_width('i', value, *bits)),
        },

        NewProc::UnsignedIntLiteral { value, bits } => match bits {
            64 => Ok(new_guint64_expr(parse_int_literal(value, 'u', *bits)?)),
            32 => Ok(new_guint32_expr(parse_int_literal(value, 'u', *bits)?)),
            16 => Ok(new_guint16_expr(parse_int_literal(value, 'u', *bits)?)),
            8 => Ok(new_guint8_expr(parse_int_literal(value, 'u', *bits)?)),
            _ => Err(unsupported_int_width('u', value, *bits)),
        },

        NewProc::BigIntLiteral(value) => {
            let s = value.trim_end_matches('n');
            let bytes = decimal_str_to_twos_complement(s)?;
            Ok(new_gbigint_expr(bytes))
        }

        NewProc::BigRatLiteral(value) => {
            let num_bytes = decimal_str_to_twos_complement(value)?;
            let den_bytes = vec![1];
            Ok(new_gbigrat_expr(num_bytes, den_bytes))
        }

        NewProc::FloatLiteral { value, bits } => match bits {
            64 => {
                let f: f64 = value.parse().map_err(|_| {
                    InterpreterError::NormalizerError(format!("Invalid float literal: {}", value))
                })?;
                Ok(new_gdouble_expr(f))
            }
            32 => {
                let f: f32 = value.parse().map_err(|_| {
                    InterpreterError::NormalizerError(format!("Invalid float literal: {}", value))
                })?;
                if f.is_infinite() {
                    return Err(InterpreterError::NormalizerError(format!(
                        "Float literal {}f32 is out of range for f32",
                        value
                    )));
                }
                Ok(new_gfloat32_expr(f))
            }
            _ => Err(InterpreterError::NormalizerError(format!(
                "Float width f{} is not supported: only f32 and f64 floats are supported, found {}f{}",
                bits, value, bits
            ))),
        },

        NewProc::FixedPointLiteral { value, scale } => {
            let unscaled_bytes = decimal_str_to_unscaled(value, *scale)?;
            Ok(new_gfixedpoint_expr(unscaled_bytes, *scale))
        }

        NewProc::StringLiteral(value) => Ok(new_gstring_expr(value.to_string())),

        NewProc::UriLiteral(uri) => {
            let uri_value = uri.to_string();
            let stripped_value = if uri_value.starts_with('`') && uri_value.ends_with('`') {
                uri_value[1..uri_value.len() - 1].to_string()
            } else {
                uri_value
            };
            Ok(new_guri_expr(stripped_value))
        }

        _ => Err(InterpreterError::BugFoundError(
            "Expected a ground type in new AST, found unsupported variant".to_string(),
        )),
    }
}

fn unsupported_int_width(sign: char, value: &str, bits: u32) -> InterpreterError {
    InterpreterError::NormalizerError(format!(
        "Integer width {sign}{bits} is not supported by this interpreter: only i32, i64, u8, u16, u32 and u64 integers are supported, found {value}{sign}{bits}"
    ))
}

fn parse_int_literal<T: std::str::FromStr>(
    value: &str,
    sign: char,
    bits: u32,
) -> Result<T, InterpreterError> {
    value.parse().map_err(|_| {
        InterpreterError::NormalizerError(format!(
            "Integer literal {value}{sign}{bits} is invalid or out of range for {sign}{bits}"
        ))
    })
}

fn decimal_str_to_twos_complement(s: &str) -> Result<Vec<u8>, InterpreterError> {
    let s = s.trim();
    if s.is_empty() || s == "0" {
        return Ok(vec![0]);
    }
    let (negative, digits) = if s.starts_with('-') {
        (true, &s[1..])
    } else if s.starts_with('+') {
        (false, &s[1..])
    } else {
        (false, s)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(InterpreterError::NormalizerError(format!(
            "Invalid decimal number: {}",
            s
        )));
    }
    let mag_bytes = decimal_to_magnitude(digits);
    let mut result = mag_bytes;
    if result[0] & 0x80 != 0 {
        result.insert(0, 0x00);
    }
    if negative {
        result = negate_tc_bytes(&result);
    }
    Ok(result)
}

fn decimal_to_magnitude(s: &str) -> Vec<u8> {
    let mut result = vec![0u8];
    for ch in s.chars() {
        let digit = ch as u8 - b'0';
        let mut carry = digit as u16;
        for byte in result.iter_mut().rev() {
            let val = (*byte as u16) * 10 + carry;
            *byte = (val & 0xFF) as u8;
            carry = val >> 8;
        }
        while carry > 0 {
            result.insert(0, (carry & 0xFF) as u8);
            carry >>= 8;
        }
    }
    if result.is_empty() {
        vec![0]
    } else {
        result
    }
}

fn negate_tc_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut result = bytes.to_vec();
    for byte in result.iter_mut() {
        *byte = !*byte;
    }
    let mut carry = true;
    for byte in result.iter_mut().rev() {
        if carry {
            let (val, overflow) = byte.overflowing_add(1);
            *byte = val;
            carry = overflow;
        }
    }
    let is_neg = result[0] & 0x80 != 0;
    let trim = if is_neg { 0xFF } else { 0x00 };
    let mut start = 0;
    while start < result.len() - 1 {
        if result[start] != trim {
            break;
        }
        if (result[start + 1] & 0x80 != 0) != is_neg {
            break;
        }
        start += 1;
    }
    result[start..].to_vec()
}

fn decimal_str_to_unscaled(s: &str, scale: u32) -> Result<Vec<u8>, InterpreterError> {
    let s = s.trim();
    let (negative, digits) = if s.starts_with('-') {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let (integer_part, frac_part) = if let Some(dot_pos) = digits.find('.') {
        (&digits[..dot_pos], &digits[dot_pos + 1..])
    } else {
        (digits, "")
    };
    let scale_usize = scale as usize;
    let padded_frac = if frac_part.len() < scale_usize {
        format!("{:0<width$}", frac_part, width = scale_usize)
    } else {
        frac_part[..scale_usize].to_string()
    };
    let unscaled_str = format!("{}{}", integer_part, padded_frac);
    let full_str = if negative {
        format!("-{}", unscaled_str)
    } else {
        unscaled_str
    };
    decimal_str_to_twos_complement(&full_str)
}

/*
 In the new engine, we don't have a separate BoolMatcher normalizer for BoolLiteral,
 which is why the tests with BoolMatcherSpec as well as with GroundMatcherSpec will be described below.

 rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/BoolMatcherSpec.scala
 rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/GroundMatcherSpec.scala
*/
#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use rholang_parser::ast::Proc;

    use crate::rust::interpreter::compiler::normalizer::ground_normalize_matcher::normalize_ground;
    use crate::rust::interpreter::errors::InterpreterError;

    #[test]
    fn bool_true_should_compile_as_gbool_true() {
        let proc = Proc::BoolLiteral(true);
        let result = normalize_ground(&proc);
        assert!(result.is_ok());
        let expr = result.unwrap();
        assert_eq!(expr.expr_instance, Some(ExprInstance::GBool(true)));
    }

    #[test]
    fn bool_false_should_compile_as_gbool_false() {
        let proc = Proc::BoolLiteral(false);
        let result = normalize_ground(&proc);
        assert!(result.is_ok());
        let expr = result.unwrap();
        assert_eq!(expr.expr_instance, Some(ExprInstance::GBool(false)));
    }

    #[test]
    fn long_should_compile_as_gint() {
        let proc = Proc::LongLiteral(42);
        let result = normalize_ground(&proc);
        assert!(result.is_ok());
        let expr = result.unwrap();
        assert_eq!(expr.expr_instance, Some(ExprInstance::GInt(42)));
    }

    #[test]
    fn string_should_compile_as_gstring() {
        let proc = Proc::StringLiteral("hello");
        let result = normalize_ground(&proc);
        assert!(result.is_ok());
        let expr = result.unwrap();
        assert_eq!(
            expr.expr_instance,
            Some(ExprInstance::GString("hello".to_string()))
        );
    }

    // TODO: URI tests omitted because Uri struct has private fields and can't be constructed in tests
    // The URI normalization logic is tested through integration tests with actual parsing

    #[test]
    fn unsupported_type_should_return_error() {
        let proc = Proc::Nil;
        let result = normalize_ground(&proc);
        assert!(matches!(result, Err(InterpreterError::BugFoundError(_))));
    }

    #[test]
    fn signed_int_with_i64_width_compiles_as_gint() {
        let proc = Proc::SignedIntLiteral {
            value: "-42",
            bits: 64,
        };
        let expr = normalize_ground(&proc).unwrap();
        assert_eq!(expr.expr_instance, Some(ExprInstance::GInt(-42)));
    }

    #[test]
    fn invalid_signed_int_is_a_normalizer_error() {
        let proc = Proc::SignedIntLiteral {
            value: "not-a-number",
            bits: 64,
        };
        assert!(matches!(
            normalize_ground(&proc),
            Err(InterpreterError::NormalizerError(_))
        ));
    }

    #[test]
    fn i64_literal_range_is_enforced() {
        for (value, expected) in [
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
        ] {
            let expr = normalize_ground(&Proc::SignedIntLiteral { value, bits: 64 }).unwrap();
            assert_eq!(expr.expr_instance, Some(ExprInstance::GInt(expected)));
        }
        match normalize_ground(&Proc::SignedIntLiteral {
            value: "9223372036854775808",
            bits: 64,
        }) {
            Err(InterpreterError::NormalizerError(msg)) => {
                assert!(msg.contains("out of range for i64"), "{msg}")
            }
            other => panic!("expected NormalizerError, got {other:?}"),
        }
    }

    #[test]
    fn sized_ints_compile_to_their_own_types() {
        let cases = [
            (
                Proc::SignedIntLiteral {
                    value: "-7",
                    bits: 32,
                },
                ExprInstance::GInt32(-7),
            ),
            (
                Proc::UnsignedIntLiteral {
                    value: "4294967295",
                    bits: 32,
                },
                ExprInstance::GUint32(u32::MAX),
            ),
            (
                Proc::UnsignedIntLiteral {
                    value: "65535",
                    bits: 16,
                },
                ExprInstance::GUint16(65535),
            ),
            (
                Proc::UnsignedIntLiteral {
                    value: "255",
                    bits: 8,
                },
                ExprInstance::GUint8(255),
            ),
        ];
        for (proc, expected) in cases {
            assert_eq!(
                normalize_ground(&proc).unwrap().expr_instance,
                Some(expected)
            );
        }
    }

    #[test]
    fn sized_int_literals_out_of_range_are_rejected() {
        let cases = [
            Proc::SignedIntLiteral {
                value: "2147483648",
                bits: 32,
            },
            Proc::UnsignedIntLiteral {
                value: "4294967296",
                bits: 32,
            },
            Proc::UnsignedIntLiteral {
                value: "65536",
                bits: 16,
            },
            Proc::UnsignedIntLiteral {
                value: "256",
                bits: 8,
            },
        ];
        for proc in cases {
            assert!(
                matches!(
                    normalize_ground(&proc),
                    Err(InterpreterError::NormalizerError(_))
                ),
                "expected NormalizerError for {proc:?}"
            );
        }
    }

    #[test]
    fn signed_ints_with_unsupported_width_are_rejected() {
        for bits in [8u32, 16, 128] {
            match normalize_ground(&Proc::SignedIntLiteral { value: "5", bits }) {
                Err(InterpreterError::NormalizerError(msg)) => {
                    assert!(msg.contains(&format!("i{bits}")), "bits {bits}: {msg}")
                }
                other => panic!("expected NormalizerError for i{bits}, got {other:?}"),
            }
        }
    }

    #[test]
    fn unsigned_int_with_u64_width_compiles_as_guint64() {
        let value = u64::MAX.to_string();
        let proc = Proc::UnsignedIntLiteral {
            value: &value,
            bits: 64,
        };
        let expr = normalize_ground(&proc).unwrap();
        assert_eq!(expr.expr_instance, Some(ExprInstance::GUint64(u64::MAX)));
    }

    #[test]
    fn invalid_unsigned_int_is_a_normalizer_error() {
        for value in ["-1", "18446744073709551616"] {
            let proc = Proc::UnsignedIntLiteral { value, bits: 64 };
            assert!(matches!(
                normalize_ground(&proc),
                Err(InterpreterError::NormalizerError(_))
            ));
        }
    }

    #[test]
    fn unsigned_ints_with_unsupported_width_are_rejected() {
        for bits in [1u32, 128] {
            match normalize_ground(&Proc::UnsignedIntLiteral { value: "5", bits }) {
                Err(InterpreterError::NormalizerError(msg)) => {
                    assert!(msg.contains(&format!("u{bits}")), "bits {bits}: {msg}")
                }
                other => panic!("expected NormalizerError for u{bits}, got {other:?}"),
            }
        }
    }

    #[test]
    fn big_int_literals_compile_to_twos_complement_bytes() {
        let cases = vec![
            ("255n", vec![0x00, 0xFF]),
            ("-1", vec![0xFF]),
            ("0", vec![0x00]),
            ("+7", vec![0x07]),
        ];
        for (literal, expected) in cases {
            let expr = normalize_ground(&Proc::BigIntLiteral(literal)).unwrap();
            assert_eq!(
                expr.expr_instance,
                Some(ExprInstance::GBigInt(expected)),
                "literal {literal}"
            );
        }

        assert!(matches!(
            normalize_ground(&Proc::BigIntLiteral("12x")),
            Err(InterpreterError::NormalizerError(_))
        ));
    }

    #[test]
    fn big_rat_literals_compile_with_denominator_one() {
        let expr = normalize_ground(&Proc::BigRatLiteral("7")).unwrap();
        match expr.expr_instance {
            Some(ExprInstance::GBigRat(rat)) => {
                assert_eq!(rat.numerator, vec![7]);
                assert_eq!(rat.denominator, vec![1]);
            }
            other => panic!("expected GBigRat, got {other:?}"),
        }
    }

    #[test]
    fn float_literals_compile_as_gdouble() {
        let proc = Proc::FloatLiteral {
            value: "2.5",
            bits: 64,
        };
        let expr = normalize_ground(&proc).unwrap();
        assert_eq!(
            expr.expr_instance,
            Some(ExprInstance::GDouble(2.5f64.to_bits()))
        );

        assert!(matches!(
            normalize_ground(&Proc::FloatLiteral {
                value: "not-a-float",
                bits: 64,
            }),
            Err(InterpreterError::NormalizerError(_))
        ));
    }

    #[test]
    fn f32_float_literals_compile_as_gfloat32() {
        let expr = normalize_ground(&Proc::FloatLiteral {
            value: "2.5",
            bits: 32,
        })
        .unwrap();
        assert_eq!(
            expr.expr_instance,
            Some(ExprInstance::GFloat32(2.5f32.to_bits()))
        );

        assert!(matches!(
            normalize_ground(&Proc::FloatLiteral {
                value: "1e39",
                bits: 32,
            }),
            Err(InterpreterError::NormalizerError(_))
        ));
    }

    #[test]
    fn float_literals_with_unsupported_width_are_rejected() {
        for bits in [16u16, 128] {
            let result = normalize_ground(&Proc::FloatLiteral { value: "1.0", bits });
            match result {
                Err(InterpreterError::NormalizerError(msg)) => {
                    assert!(msg.contains(&format!("f{bits}")), "bits {bits}: {msg}")
                }
                other => panic!("expected NormalizerError for f{bits}, got {other:?}"),
            }
        }
    }

    #[test]
    fn fixed_point_literals_compile_with_scaled_unscaled_bytes() {
        let cases = vec![
            ("1.23", 2u32, vec![0x7B]),
            ("5", 0u32, vec![0x05]),
            ("-0.05", 2u32, vec![0xFB]),
            ("1.2", 2u32, vec![0x78]),
        ];
        for (literal, scale, expected_unscaled) in cases {
            let proc = Proc::FixedPointLiteral {
                value: literal,
                scale,
            };
            let expr = normalize_ground(&proc).unwrap();
            match expr.expr_instance {
                Some(ExprInstance::GFixedPoint(fp)) => {
                    assert_eq!(fp.unscaled, expected_unscaled, "literal {literal}");
                    assert_eq!(fp.scale, scale, "literal {literal}");
                }
                other => panic!("expected GFixedPoint for {literal}, got {other:?}"),
            }
        }
    }
}
