//! D-S2 (DR-95): the rhoapi types are closed history types. Their serde
//! derives decode only vectors, options, one B-tree map (`New.injections`),
//! strings, byte buffers, scalars and enums of these: the generated types
//! have no box, shared pointer, hash table or B-tree set, and no custom
//! `Deserialize` implementation. Some fields carry a `serialize_with`
//! attribute, which changes only how the field is encoded.

use shared::rust::closed_decode::ClosedDecode;

use crate::rhoapi::*;

macro_rules! closed_decode {
    ($($ty:ty),* $(,)?) => {
        // SAFETY: each listed type is a prost message or oneof enum whose serde
        // derive decodes only the shapes that `ClosedDecode` admits (see the
        // module documentation).
        $(unsafe impl ClosedDecode for $ty {})*
    };
}

closed_decode!(
    Par,
    CostAuthority,
    CostRegion,
    CostSignature,
    CostSignatureCompound,
    CostSignedTerm,
    CostStack,
    ListParWithRandom,
    BindPattern,
    ParWithRandom,
    TaggedContinuation,
    tagged_continuation::TaggedCont,
    Send,
    Receive,
    ReceiveBind,
    New,
    Match,
    MatchCase,
    If,
    Bundle,
    Expr,
    Connective,
    ConnectiveBody,
    GUnforgeable,
    GPrivate,
    GDeployId,
    GDeployerId,
    GAuthorityId,
    GPrincipalId,
    GSysAuthToken,
    Var,
    VarRef,
    GBigRational,
    GFixedPoint,
    EList,
    ETuple,
    ESet,
    EMap,
    KeyValuePair,
    EPathMap,
    EZipper,
    EMethod,
    EVar,
    ENot,
    ENeg,
    EMult,
    EDiv,
    EPlus,
    EMinus,
    ELt,
    ELte,
    EGt,
    EGte,
    EEq,
    ENeq,
    EAnd,
    EOr,
    EMatches,
    EPercentPercent,
    EPlusPlus,
    EMinusMinus,
    EMod,
    var::VarInstance,
    var::WildcardMsg,
    cost_signature::Value,
    expr::ExprInstance,
    connective::ConnectiveInstance,
    g_unforgeable::UnfInstance,
);
