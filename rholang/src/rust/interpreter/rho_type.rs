// See rholang/src/main/scala/coop/rchain/rholang/interpreter/RhoType.scala

use std::collections::HashMap;
use std::hash::Hash;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    EList, ETuple, Expr, GDeployId, GDeployerId, GPrincipalId, GPrivate, GSysAuthToken,
    GUnforgeable, Par,
};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::rholang::implicits::{single_expr, single_unforgeable};
use models::rust::sorted_par_map::SortedParMap;
use rspace_plus_plus::rspace::history::Either;

pub struct RhoNil;

impl RhoNil {
    pub fn unapply(p: &Par) -> bool { p.is_nil() }

    pub fn create_par() -> Par { Par::default() }
}

pub struct RhoByteArray;

impl RhoByteArray {
    pub fn unapply(p: &Par) -> Option<Vec<u8>> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::GByteArray(bs)),
            } = expr
            {
                return Some(bs);
            }
        }
        None
    }

    pub fn create_par(bytes: Vec<u8>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(bytes)),
        }])
    }
}

pub struct RhoString;

impl RhoString {
    pub fn unapply(p: &Par) -> Option<String> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::GString(str)),
            } = expr
            {
                return Some(str);
            }
        }
        None
    }

    pub fn create_par(s: String) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(s)),
        }])
    }
}

pub struct RhoBoolean;

impl RhoBoolean {
    pub fn create_par(b: bool) -> Par { Par::default().with_exprs(vec![Self::create_expr(b)]) }

    pub fn create_expr(b: bool) -> Expr {
        Expr {
            expr_instance: Some(ExprInstance::GBool(b)),
        }
    }

    pub fn unapply(p: &Par) -> Option<bool> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::GBool(b)),
            } = expr
            {
                return Some(b);
            }
        }
        None
    }
}

pub struct RhoNumber;

impl RhoNumber {
    pub fn create_expr(i: i64) -> Expr {
        Expr {
            expr_instance: Some(ExprInstance::GInt(i)),
        }
    }

    pub fn create_par(i: i64) -> Par { Par::default().with_exprs(vec![RhoNumber::create_expr(i)]) }

    pub fn unapply(p: &Par) -> Option<i64> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::GInt(v)),
            } = expr
            {
                return Some(v);
            }
        }
        None
    }
}

pub struct RhoTuple2;

impl RhoTuple2 {
    pub fn create_par(tuple: (Par, Par)) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![tuple.0, tuple.1],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }

    pub fn unapply(p: &Par) -> Option<(Par, Par)> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple { ps, .. })),
            } = expr
            {
                if ps.len() == 2 {
                    return Some((ps[0].clone(), ps[1].clone()));
                } else {
                    return None;
                }
            }
        }
        None
    }
}

pub struct RhoList;

impl RhoList {
    pub fn create_par(list: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: list,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    pub fn unapply(p: &Par) -> Option<Vec<Par>> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
            } = expr
            {
                return Some(ps);
            }
        }
        None
    }
}

pub struct RhoMap;

impl RhoMap {
    pub fn create_par(hash_map: HashMap<Par, Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
                ParMap::create_from_sorted_par_map(SortedParMap::create_from_map(hash_map)),
            ))),
        }])
    }

    pub fn unapply(p: &Par) -> Option<HashMap<Par, Par>> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::EMapBody(emap)),
            } = expr
            {
                return Some(ParMapTypeMapper::emap_to_par_map(emap).ps.ps);
            }
        }
        None
    }
}

pub struct RhoUri;

impl RhoUri {
    pub fn create_par(s: String) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GUri(s)),
        }])
    }

    pub fn unapply(p: &Par) -> Option<String> {
        if let Some(expr) = single_expr(p) {
            if let Expr {
                expr_instance: Some(ExprInstance::GUri(s)),
            } = expr
            {
                return Some(s);
            }
        }
        None
    }
}

pub struct RhoDeployerId;

impl RhoDeployerId {
    pub fn create_par(bytes: Vec<u8>) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: bytes,
            })),
        }])
    }

    pub fn unapply(p: &Par) -> Option<Vec<u8>> {
        if let Some(expr) = single_unforgeable(p) {
            if let GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(id)),
            } = expr
            {
                return Some(id.public_key);
            }
        }
        None
    }
}

pub struct RhoSingleCustodyId;

impl RhoSingleCustodyId {
    pub fn unapply(p: &Par) -> Option<Vec<u8>> {
        match single_unforgeable(p)?.unf_instance? {
            UnfInstance::GDeployerIdBody(id) => Some(id.public_key),
            UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: 1,
                public_key,
            }) => Some(public_key),
            _ => None,
        }
    }
}

pub struct RhoDeployId;

impl RhoDeployId {
    pub fn create_par(bytes: Vec<u8>) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: bytes })),
        }])
    }

    pub fn unapply(p: &Par) -> Option<Vec<u8>> {
        if let Some(expr) = single_unforgeable(p) {
            if let GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(id)),
            } = expr
            {
                return Some(id.sig);
            }
        }
        None
    }
}

pub struct RhoName;

impl RhoName {
    pub fn create_par(gprivate: GPrivate) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(gprivate)),
        }])
    }

    pub fn unapply(p: &Par) -> Option<GPrivate> {
        if let Some(expr) = single_unforgeable(p) {
            if let GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(gprivate)),
            } = expr
            {
                return Some(gprivate);
            }
        }
        None
    }
}

pub struct RhoExpression;

impl RhoExpression {
    pub fn create_par(expr: Expr) -> Par { Par::default().with_exprs(vec![expr]) }

    pub fn unapply(p: &Par) -> Option<Expr> { single_expr(p) }
}

pub struct RhoUnforgeable;

impl RhoUnforgeable {
    pub fn create_par(unforgeable: GUnforgeable) -> Par {
        Par::default().with_unforgeables(vec![unforgeable])
    }

    pub fn unapply(p: &Par) -> Option<GUnforgeable> { single_unforgeable(p) }
}

pub struct RhoSysAuthToken;

impl RhoSysAuthToken {
    pub fn create_par(token: GSysAuthToken) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(token)),
        }])
    }

    pub fn unapply(p: &Par) -> Option<GSysAuthToken> {
        if let Some(expr) = single_unforgeable(p) {
            if let GUnforgeable {
                unf_instance: Some(UnfInstance::GSysAuthTokenBody(token)),
            } = expr
            {
                return Some(token);
            }
        }
        None
    }
}

pub trait Extractor {
    type RustType;

    fn unapply(p: &Par) -> Option<Self::RustType>;
}

impl Extractor for RhoBoolean {
    type RustType = bool;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoBoolean::unapply(p) }
}

impl Extractor for RhoString {
    type RustType = String;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoString::unapply(p) }
}

impl Extractor for RhoNil {
    type RustType = ();

    fn unapply(p: &Par) -> Option<Self::RustType> {
        if RhoNil::unapply(p) {
            Some(())
        } else {
            None
        }
    }
}

impl Extractor for RhoByteArray {
    type RustType = Vec<u8>;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoByteArray::unapply(p) }
}

impl Extractor for RhoDeployerId {
    type RustType = Vec<u8>;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoDeployerId::unapply(p) }
}

impl Extractor for RhoSingleCustodyId {
    type RustType = Vec<u8>;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoSingleCustodyId::unapply(p) }
}

impl Extractor for RhoDeployId {
    type RustType = Vec<u8>;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoDeployId::unapply(p) }
}

impl Extractor for RhoName {
    type RustType = GPrivate;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoName::unapply(p) }
}

impl Extractor for RhoNumber {
    type RustType = i64;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoNumber::unapply(p) }
}

impl Extractor for RhoUri {
    type RustType = String;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoUri::unapply(p) }
}

impl Extractor for RhoUnforgeable {
    type RustType = GUnforgeable;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoUnforgeable::unapply(p) }
}

impl Extractor for RhoExpression {
    type RustType = Expr;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoExpression::unapply(p) }
}

impl Extractor for RhoSysAuthToken {
    type RustType = GSysAuthToken;

    fn unapply(p: &Par) -> Option<Self::RustType> { RhoSysAuthToken::unapply(p) }
}

impl<A, B> Extractor for (A, B)
where
    A: Extractor,
    B: Extractor,
{
    type RustType = (A::RustType, B::RustType);

    fn unapply(p: &Par) -> Option<Self::RustType> {
        if let Some((p1, p2)) = RhoTuple2::unapply(p) {
            if let (Some(a), Some(b)) = (A::unapply(&p1), B::unapply(&p2)) {
                return Some((a, b));
            }
        }
        None
    }
}

impl<A> Extractor for Vec<A>
where A: Extractor
{
    type RustType = Vec<A::RustType>;

    fn unapply(p: &Par) -> Option<Self::RustType> {
        if let Some(plist) = RhoList::unapply(p) {
            return plist.into_iter().map(|par| A::unapply(&par)).collect();
        }
        None
    }
}

impl<A, B> Extractor for HashMap<A, B>
where
    A: Extractor,
    B: Extractor,
    A::RustType: Eq + Hash,
{
    type RustType = HashMap<A::RustType, B::RustType>;

    fn unapply(p: &Par) -> Option<Self::RustType> {
        if let Some(pmap) = RhoMap::unapply(p) {
            return pmap
                .into_iter()
                .map(
                    |(pkey, pvalue)| match (A::unapply(&pkey), B::unapply(&pvalue)) {
                        (Some(key), Some(value)) => Some((key, value)),
                        _ => None,
                    },
                )
                .collect();
        }
        None
    }
}

impl<A, B> Extractor for Either<A, B>
where
    A: Extractor,
    B: Extractor,
{
    type RustType = Either<A::RustType, B::RustType>;

    fn unapply(p: &Par) -> Option<Self::RustType> {
        if let Some(b) = B::unapply(p) {
            Some(Either::Right(b))
        } else {
            A::unapply(p).map(Either::Left)
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::g_unforgeable::UnfInstance::{GAuthorityIdBody, GPrincipalIdBody};
    use models::rhoapi::{GAuthorityId, GPrincipalId};
    use proptest::prelude::*;

    use super::*;

    fn unforgeable(instance: UnfInstance) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(instance),
        }])
    }

    proptest! {
        #[test]
        fn legacy_and_v61_singleton_custody_project_the_same_key(public_key in prop::collection::vec(any::<u8>(), 1..130)) {
            let legacy = RhoDeployerId::create_par(public_key.clone());
            let principal = unforgeable(GPrincipalIdBody(GPrincipalId {
                key_family: 1,
                public_key: public_key.clone(),
            }));

            prop_assert_eq!(RhoSingleCustodyId::unapply(&legacy), Some(public_key.clone()));
            prop_assert_eq!(RhoSingleCustodyId::unapply(&principal), Some(public_key));
        }

        #[test]
        fn non_custody_key_families_are_rejected(public_key in prop::collection::vec(any::<u8>(), 1..130), key_family in 2u32..=u32::MAX) {
            let principal = unforgeable(GPrincipalIdBody(GPrincipalId {
                key_family,
                public_key,
            }));

            prop_assert_eq!(RhoSingleCustodyId::unapply(&principal), None);
        }
    }

    #[test]
    fn compound_authority_is_not_a_single_custody_capability() {
        let authority = unforgeable(GAuthorityIdBody(GAuthorityId { id: vec![7; 32] }));

        assert_eq!(RhoSingleCustodyId::unapply(&authority), None);
    }

    #[test]
    fn nil_round_trip_and_negative() {
        assert!(RhoNil::unapply(&RhoNil::create_par()));
        assert!(!RhoNil::unapply(&RhoNumber::create_par(1)));
        assert_eq!(
            <RhoNil as Extractor>::unapply(&RhoNil::create_par()),
            Some(())
        );
        assert_eq!(
            <RhoNil as Extractor>::unapply(&RhoNumber::create_par(1)),
            None
        );
    }

    #[test]
    fn byte_array_round_trip_and_negative() {
        let par = RhoByteArray::create_par(vec![1, 2, 3]);
        assert_eq!(RhoByteArray::unapply(&par), Some(vec![1, 2, 3]));
        assert_eq!(RhoByteArray::unapply(&RhoNumber::create_par(1)), None);
        assert_eq!(
            <RhoByteArray as Extractor>::unapply(&par),
            Some(vec![1, 2, 3])
        );
    }

    #[test]
    fn string_round_trip_and_negative() {
        let par = RhoString::create_par("hi".to_string());
        assert_eq!(RhoString::unapply(&par), Some("hi".to_string()));
        assert_eq!(RhoString::unapply(&RhoNumber::create_par(1)), None);
        assert_eq!(
            <RhoString as Extractor>::unapply(&par),
            Some("hi".to_string())
        );
    }

    #[test]
    fn boolean_round_trip_and_negative() {
        let par = RhoBoolean::create_par(true);
        assert_eq!(RhoBoolean::unapply(&par), Some(true));
        assert_eq!(RhoBoolean::unapply(&RhoNumber::create_par(1)), None);
        assert_eq!(
            RhoBoolean::create_expr(false).expr_instance,
            Some(ExprInstance::GBool(false))
        );
        assert_eq!(<RhoBoolean as Extractor>::unapply(&par), Some(true));
    }

    #[test]
    fn number_round_trip_and_negative() {
        let par = RhoNumber::create_par(42);
        assert_eq!(RhoNumber::unapply(&par), Some(42));
        assert_eq!(RhoNumber::unapply(&RhoBoolean::create_par(true)), None);
        assert_eq!(<RhoNumber as Extractor>::unapply(&par), Some(42));
    }

    #[test]
    fn tuple2_round_trip_and_negatives() {
        let par = RhoTuple2::create_par((
            RhoNumber::create_par(1),
            RhoString::create_par("a".to_string()),
        ));
        assert_eq!(
            RhoTuple2::unapply(&par),
            Some((
                RhoNumber::create_par(1),
                RhoString::create_par("a".to_string())
            ))
        );
        assert_eq!(RhoTuple2::unapply(&RhoNumber::create_par(1)), None);

        let triple = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![Par::default(), Par::default(), Par::default()],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        assert_eq!(RhoTuple2::unapply(&triple), None);

        assert_eq!(
            <(RhoNumber, RhoString) as Extractor>::unapply(&par),
            Some((1, "a".to_string()))
        );
        assert_eq!(<(RhoNumber, RhoNumber) as Extractor>::unapply(&par), None);
    }

    #[test]
    fn list_round_trip_and_extractor() {
        let par = RhoList::create_par(vec![RhoNumber::create_par(1), RhoNumber::create_par(2)]);
        assert_eq!(
            RhoList::unapply(&par),
            Some(vec![RhoNumber::create_par(1), RhoNumber::create_par(2)])
        );
        assert_eq!(RhoList::unapply(&RhoNumber::create_par(1)), None);
        assert_eq!(
            <Vec<RhoNumber> as Extractor>::unapply(&par),
            Some(vec![1, 2])
        );

        let mixed =
            RhoList::create_par(vec![RhoNumber::create_par(1), RhoBoolean::create_par(true)]);
        assert_eq!(<Vec<RhoNumber> as Extractor>::unapply(&mixed), None);
    }

    #[test]
    fn map_round_trip_and_extractor() {
        let mut source = HashMap::new();
        source.insert(
            RhoString::create_par("k".to_string()),
            RhoNumber::create_par(7),
        );
        let par = RhoMap::create_par(source.clone());
        assert_eq!(RhoMap::unapply(&par), Some(source));
        assert_eq!(RhoMap::unapply(&RhoNumber::create_par(1)), None);

        let mut expected = HashMap::new();
        expected.insert("k".to_string(), 7i64);
        assert_eq!(
            <HashMap<RhoString, RhoNumber> as Extractor>::unapply(&par),
            Some(expected)
        );
        assert_eq!(
            <HashMap<RhoString, RhoString> as Extractor>::unapply(&par),
            None
        );
    }

    #[test]
    fn uri_round_trip_and_negative() {
        let par = RhoUri::create_par("rho:io:stdout".to_string());
        assert_eq!(RhoUri::unapply(&par), Some("rho:io:stdout".to_string()));
        assert_eq!(RhoUri::unapply(&RhoNumber::create_par(1)), None);
        assert_eq!(
            <RhoUri as Extractor>::unapply(&par),
            Some("rho:io:stdout".to_string())
        );
    }

    #[test]
    fn deployer_id_and_deploy_id_round_trips() {
        let deployer = RhoDeployerId::create_par(vec![1, 2]);
        assert_eq!(RhoDeployerId::unapply(&deployer), Some(vec![1, 2]));
        assert_eq!(
            RhoDeployerId::unapply(&RhoDeployId::create_par(vec![1])),
            None
        );
        assert_eq!(
            <RhoDeployerId as Extractor>::unapply(&deployer),
            Some(vec![1, 2])
        );

        let deploy = RhoDeployId::create_par(vec![3, 4]);
        assert_eq!(RhoDeployId::unapply(&deploy), Some(vec![3, 4]));
        assert_eq!(RhoDeployId::unapply(&deployer), None);
        assert_eq!(
            <RhoDeployId as Extractor>::unapply(&deploy),
            Some(vec![3, 4])
        );
    }

    #[test]
    fn name_round_trip_and_negative() {
        let gprivate = GPrivate { id: vec![9, 9] };
        let par = RhoName::create_par(gprivate.clone());
        assert_eq!(RhoName::unapply(&par), Some(gprivate.clone()));
        assert_eq!(RhoName::unapply(&RhoDeployId::create_par(vec![1])), None);
        assert_eq!(<RhoName as Extractor>::unapply(&par), Some(gprivate));
    }

    #[test]
    fn expression_and_unforgeable_round_trips() {
        let expr = RhoNumber::create_expr(5);
        let par = RhoExpression::create_par(expr.clone());
        assert_eq!(RhoExpression::unapply(&par), Some(expr.clone()));
        assert_eq!(<RhoExpression as Extractor>::unapply(&par), Some(expr));

        let unforgeable = GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1] })),
        };
        let unf_par = RhoUnforgeable::create_par(unforgeable.clone());
        assert_eq!(RhoUnforgeable::unapply(&unf_par), Some(unforgeable.clone()));
        assert_eq!(RhoUnforgeable::unapply(&Par::default()), None);
        assert_eq!(
            <RhoUnforgeable as Extractor>::unapply(&unf_par),
            Some(unforgeable)
        );
    }

    #[test]
    fn sys_auth_token_round_trip_and_negative() {
        let par = RhoSysAuthToken::create_par(GSysAuthToken::default());
        assert_eq!(
            RhoSysAuthToken::unapply(&par),
            Some(GSysAuthToken::default())
        );
        assert_eq!(
            RhoSysAuthToken::unapply(&RhoDeployId::create_par(vec![1])),
            None
        );
        assert_eq!(
            <RhoSysAuthToken as Extractor>::unapply(&par),
            Some(GSysAuthToken::default())
        );
    }

    #[test]
    fn either_extractor_prefers_the_right_side() {
        let number = RhoNumber::create_par(3);
        let text = RhoString::create_par("t".to_string());

        assert_eq!(
            <Either<RhoString, RhoNumber> as Extractor>::unapply(&number),
            Some(Either::Right(3))
        );
        assert_eq!(
            <Either<RhoString, RhoNumber> as Extractor>::unapply(&text),
            Some(Either::Left("t".to_string()))
        );
        assert_eq!(
            <Either<RhoString, RhoNumber> as Extractor>::unapply(&RhoBoolean::create_par(true)),
            None
        );
    }
}
