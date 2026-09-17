use std::num::NonZeroUsize;

use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoList, RhoNumber};

use crate::rust::errors::CasperError;

pub fn query_source(scope: &[u8; 32]) -> String {
    format!(
        r#"new return, rl(`rho:registry:lookup`), vaultCh in {{
          rl!(`rho:vault:system`, *vaultCh) |
          for (@(_, vault) <- vaultCh) {{
            @vault!("costCursor", "{}".hexToBytes(), *return)
          }}
        }}"#,
        hex::encode(scope),
    )
}

pub fn decode_snapshot(
    values: &[Par],
    payer_count: NonZeroUsize,
) -> Result<Option<MonetaryCursor>, CasperError> {
    let invalid = || {
        CasperError::InvalidCostSettlement(
            "invalid SystemVault monetary cursor response".to_string(),
        )
    };
    let [value] = values else {
        return Err(invalid());
    };
    let fields = RhoList::unapply(value).ok_or_else(invalid)?;
    let [presence, revision, position] = fields.as_slice() else {
        return Err(invalid());
    };
    let present = RhoBoolean::unapply(presence).ok_or_else(invalid)?;
    let revision = RhoNumber::unapply(revision).ok_or_else(invalid)?;
    let position = RhoNumber::unapply(position).ok_or_else(invalid)?;
    let canonical = RhoList::create_par(vec![
        RhoBoolean::create_par(present),
        RhoNumber::create_par(revision),
        RhoNumber::create_par(position),
    ]);
    if *value != canonical {
        return Err(invalid());
    }
    if present {
        MonetaryCursor::new(revision, position, payer_count)
            .map(Some)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))
    } else if revision == 0 && position == 0 {
        Ok(None)
    } else {
        Err(invalid())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_type::RhoNil;

    use super::*;

    fn response(present: bool, revision: i64, position: i64) -> Par {
        RhoList::create_par(vec![
            RhoBoolean::create_par(present),
            RhoNumber::create_par(revision),
            RhoNumber::create_par(position),
        ])
    }

    #[test]
    fn monetary_cursor_snapshot_distinguishes_absence_and_initialized_zero() {
        let count = NonZeroUsize::new(2).unwrap();
        assert!(decode_snapshot(&[response(false, 0, 0)], count)
            .unwrap()
            .is_none());
        assert_eq!(
            decode_snapshot(&[response(true, 0, 0)], count).unwrap(),
            Some(MonetaryCursor::INITIAL)
        );
        assert!(Compiler::source_to_adt(&query_source(&[0xff; 32])).is_ok());
    }

    #[test]
    fn monetary_cursor_snapshot_rejects_extra_missing_and_wrongly_typed_fields() {
        let count = NonZeroUsize::new(2).unwrap();
        let valid = response(true, 1, 1);
        for values in [
            vec![],
            vec![valid.clone(), valid.clone()],
            vec![valid.clone(), RhoNil::create_par()],
            vec![RhoNil::create_par()],
        ] {
            assert!(decode_snapshot(&values, count).is_err());
        }
        let fields = RhoList::unapply(&valid).unwrap();
        for index in 0..3 {
            let mut missing = fields.clone();
            missing.remove(index);
            assert!(decode_snapshot(&[RhoList::create_par(missing)], count).is_err());
            let mut wrong = fields.clone();
            wrong[index] = RhoNil::create_par();
            assert!(decode_snapshot(&[RhoList::create_par(wrong)], count).is_err());
        }
        let mut extra = fields;
        extra.push(RhoNumber::create_par(0));
        assert!(decode_snapshot(&[RhoList::create_par(extra)], count).is_err());
        for values in [
            response(false, 1, 0),
            response(false, 0, 1),
            response(true, -1, 0),
            response(true, 0, -1),
            response(true, 0, 2),
        ] {
            assert!(decode_snapshot(&[values], count).is_err());
        }
    }

    #[test]
    fn monetary_cursor_snapshot_rejects_parallel_names_in_response_and_fields() {
        let count = NonZeroUsize::new(2).unwrap();
        let extra = models::rhoapi::GUnforgeable {
            unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                models::rhoapi::GPrivate { id: vec![9; 32] },
            )),
        };
        let mut value = response(true, 1, 1);
        value.unforgeables.push(extra.clone());
        assert!(decode_snapshot(&[value], count).is_err());
        for index in 0..3 {
            let mut fields = RhoList::unapply(&response(true, 1, 1)).unwrap();
            fields[index].unforgeables.push(extra.clone());
            assert!(decode_snapshot(&[RhoList::create_par(fields)], count).is_err());
        }
    }

    #[test]
    fn monetary_cursor_snapshot_rejects_noncanonical_list_metadata() {
        let count = NonZeroUsize::new(2).unwrap();
        for mutation in 0..2 {
            let mut value = response(true, 1, 1);
            let Some(models::rhoapi::expr::ExprInstance::EListBody(list)) =
                value.exprs[0].expr_instance.as_mut()
            else {
                unreachable!();
            };
            match mutation {
                0 => list.remainder = Some(models::rhoapi::Var::default()),
                1 => list.connective_used = true,
                _ => unreachable!(),
            }
            assert!(decode_snapshot(&[value], count).is_err());
        }
    }

    #[test]
    fn monetary_cursor_snapshot_preserves_transient_cache_equality() {
        let count = NonZeroUsize::new(2).unwrap();
        let original = response(true, 1, 1);
        let mut cached = original.clone();
        cached.locally_free = vec![1];
        let Some(models::rhoapi::expr::ExprInstance::EListBody(list)) =
            cached.exprs[0].expr_instance.as_mut()
        else {
            unreachable!();
        };
        list.locally_free = vec![2];
        for field in &mut list.ps {
            field.locally_free = vec![3];
        }
        assert_eq!(cached, original);
        assert_eq!(
            decode_snapshot(&[cached], count).unwrap(),
            decode_snapshot(&[original], count).unwrap()
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn monetary_cursor_snapshot_shape_matches_the_formal_extra_content_guard(
            count in 1_usize..=1024,
            revision in 0_i64..=i64::MAX,
            position_seed in any::<u64>(),
            present in any::<bool>(),
            extra_mask in 0_u8..16,
        ) {
            let revision = if present { revision } else { 0 };
            let position = if present { (position_seed % count as u64) as i64 } else { 0 };
            let mut fields = RhoList::unapply(&response(present, revision, position)).unwrap();
            let extra = models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                    models::rhoapi::GPrivate { id: vec![9; 32] },
                )),
            };
            for (index, field) in fields.iter_mut().enumerate() {
                if extra_mask & (1 << (index + 1)) != 0 {
                    field.unforgeables.push(extra.clone());
                }
            }
            let mut value = RhoList::create_par(fields);
            if extra_mask & 1 != 0 {
                value.unforgeables.push(extra);
            }
            let decoded = decode_snapshot(&[value], NonZeroUsize::new(count).unwrap());
            prop_assert_eq!(decoded.is_ok(), extra_mask == 0);
        }

        #[test]
        fn monetary_cursor_snapshot_acceptance_matches_exact_native_bounds(
            count in 1_usize..=1024,
            revision in prop_oneof![Just(0), Just(i64::MAX), any::<i64>()],
            position in prop_oneof![Just(0), 0_i64..1024, any::<i64>()],
            present in any::<bool>(),
        ) {
            let decoded = decode_snapshot(&[response(present, revision, position)], NonZeroUsize::new(count).unwrap());
            let valid = if present { revision >= 0 && position >= 0 && (position as u64) < count as u64 }
                        else { revision == 0 && position == 0 };
            prop_assert_eq!(decoded.is_ok(), valid);
            if let Ok(Some(cursor)) = decoded {
                prop_assert!(present);
                prop_assert_eq!(cursor.revision(), revision);
                prop_assert_eq!(cursor.position(), position);
            }
        }
    }
}
