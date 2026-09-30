use shared::rust::clone_backing::{BackingError, CloneBacking, Walker};

use crate::rhoapi::*;

macro_rules! fields {
    ($ty:ident { $($field:ident),* $(,)? }) => {
        impl CloneBacking for $ty {
            fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
                let Self { $($field),* } = self;
                $(walker.push($field)?;)*
                Ok(())
            }
        }
    };
}

macro_rules! variants {
    ($ty:path, $($variant:ident),+ $(,)?) => {
        impl CloneBacking for $ty {
            fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
                match self { $(Self::$variant(value) => walker.push(value)),+ }
            }
        }
    };
}

fields!(Par {
    sends,
    receives,
    news,
    exprs,
    matches,
    unforgeables,
    bundles,
    connectives,
    conditionals,
    locally_free,
    connective_used,
    cost_signed_terms,
    cost_stacks
});
fields!(CostAuthority { regions });
fields!(CostRegion {
    instance_id,
    signature
});
fields!(CostSignature { value });
fields!(CostSignatureCompound { elements });
fields!(CostSignedTerm { body, signature });
fields!(CostStack { cells });
fields!(ListParWithRandom {
    pars,
    random_state,
    cost_authority,
    cost_stack
});
fields!(BindPattern {
    patterns,
    remainder,
    free_count
});
fields!(ParWithRandom { body, random_state });
fields!(TaggedContinuation {
    tagged_cont,
    guard,
    cost_authority
});
variants!(tagged_continuation::TaggedCont, ParBody, ScalaBodyRef);
fields!(Send {
    chan,
    data,
    persistent,
    locally_free,
    connective_used
});
fields!(Receive {
    binds,
    body,
    persistent,
    peek,
    bind_count,
    locally_free,
    connective_used,
    condition
});
fields!(ReceiveBind {
    patterns,
    source,
    remainder,
    free_count,
    cost_signature
});
fields!(New {
    bind_count,
    p,
    uri,
    injections,
    locally_free
});
fields!(Match {
    target,
    cases,
    locally_free,
    connective_used
});
fields!(MatchCase {
    pattern,
    source,
    free_count,
    guard
});
fields!(If {
    condition,
    if_true,
    if_false,
    locally_free,
    connective_used
});
fields!(Bundle {
    body,
    write_flag,
    read_flag
});
fields!(Expr { expr_instance });
fields!(Connective {
    connective_instance
});
fields!(ConnectiveBody { ps });
fields!(GUnforgeable { unf_instance });
fields!(GPrivate { id });
fields!(GDeployId { sig });
fields!(GDeployerId { public_key });
fields!(GAuthorityId { id });
fields!(GPrincipalId {
    key_family,
    public_key
});
fields!(Var { var_instance });
fields!(VarRef { index, depth });
fields!(GBigRational {
    numerator,
    denominator
});
fields!(GFixedPoint { unscaled, scale });
fields!(EList {
    ps,
    locally_free,
    connective_used,
    remainder
});
fields!(ETuple {
    ps,
    locally_free,
    connective_used
});
fields!(ESet {
    ps,
    locally_free,
    connective_used,
    remainder
});
fields!(EMap {
    kvs,
    locally_free,
    connective_used,
    remainder
});
fields!(KeyValuePair { key, value });
fields!(EPathMap {
    ps,
    locally_free,
    connective_used,
    remainder
});
fields!(EZipper {
    pathmap,
    current_path,
    is_write_zipper,
    locally_free,
    connective_used
});
fields!(EMethod {
    method_name,
    target,
    arguments,
    locally_free,
    connective_used
});
fields!(EVar { v });
fields!(ENot { p });
fields!(ENeg { p });
fields!(EMult { p1, p2 });
fields!(EDiv { p1, p2 });
fields!(EPlus { p1, p2 });
fields!(EMinus { p1, p2 });
fields!(ELt { p1, p2 });
fields!(ELte { p1, p2 });
fields!(EGt { p1, p2 });
fields!(EGte { p1, p2 });
fields!(EEq { p1, p2 });
fields!(ENeq { p1, p2 });
fields!(EAnd { p1, p2 });
fields!(EOr { p1, p2 });
fields!(EMatches { target, pattern });
fields!(EPercentPercent { p1, p2 });
fields!(EPlusPlus { p1, p2 });
fields!(EMinusMinus { p1, p2 });
fields!(EMod { p1, p2 });

variants!(
    cost_signature::Value,
    Ground,
    BoundLevel,
    Quote,
    Compound,
    Name,
    Unit
);
variants!(var::VarInstance, BoundVar, FreeVar, Wildcard);
variants!(
    expr::ExprInstance,
    GBool,
    GInt,
    GString,
    GUri,
    GByteArray,
    ENotBody,
    ENegBody,
    EMultBody,
    EDivBody,
    EPlusBody,
    EMinusBody,
    ELtBody,
    ELteBody,
    EGtBody,
    EGteBody,
    EEqBody,
    ENeqBody,
    EAndBody,
    EOrBody,
    EVarBody,
    EListBody,
    ETupleBody,
    ESetBody,
    EMapBody,
    EMethodBody,
    EPathmapBody,
    EZipperBody,
    EMatchesBody,
    EPercentPercentBody,
    EPlusPlusBody,
    EMinusMinusBody,
    EModBody,
    GDouble,
    GBigInt,
    GBigRat,
    GFixedPoint
);
variants!(
    connective::ConnectiveInstance,
    ConnAndBody,
    ConnOrBody,
    ConnNotBody,
    VarRefBody,
    ConnBool,
    ConnInt,
    ConnString,
    ConnUri,
    ConnByteArray
);
variants!(
    g_unforgeable::UnfInstance,
    GPrivateBody,
    GDeployIdBody,
    GDeployerIdBody,
    GSysAuthTokenBody,
    GAuthorityIdBody,
    GPrincipalIdBody
);

impl CloneBacking for GSysAuthToken {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {} = self;
        Ok(())
    }
}

impl CloneBacking for var::WildcardMsg {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {} = self;
        Ok(())
    }
}
