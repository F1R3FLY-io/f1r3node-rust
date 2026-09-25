use std::collections::{BTreeMap, HashMap};
use std::mem::size_of;
use std::sync::Arc;

use models::rhoapi::*;
use models::rust::host_work::HostWorkDimension;
use shared::rust::collection_backing::{hash_backing, tree_backing};

use super::index::reserve_vector;
use super::recording::work;
use super::{HostWorkBudget, InterpreterError};
use crate::rust::interpreter::accounting::authority::{AuthorityStackBirth, ResourceMultiset};
use crate::rust::interpreter::accounting::AuthorityRuntimeEvent;

pub(crate) trait CloneBacking {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError>;

    fn inline() -> bool
    where Self: Sized {
        false
    }
}

pub(crate) struct Walker<'a> {
    pending: Vec<&'a dyn CloneBacking>,
    capacity: usize,
    host: &'a HostWorkBudget,
    copy_payload: bool,
}

impl<'a> Walker<'a> {
    fn push<T: CloneBacking>(&mut self, value: &'a T) -> Result<(), InterpreterError> {
        work(self.host, HostWorkDimension::VerificationOperations, 3)?;
        work(
            self.host,
            HostWorkDimension::VerificationBytes,
            size_of::<T>()
                .checked_mul(3)
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
        if !T::inline() {
            reserve_vector(&mut self.pending, &mut self.capacity, 1, self.host)?;
            self.pending.push(value);
        }
        Ok(())
    }

    fn allocation(&self, bytes: usize) -> Result<(), InterpreterError> {
        if self.copy_payload {
            work(self.host, HostWorkDimension::SearchStateBytes, bytes)?;
        }
        work(
            self.host,
            HostWorkDimension::VerificationBytes,
            bytes
                .checked_mul(2)
                .ok_or(InterpreterError::HostWorkRejected)?,
        )
    }

    fn slice<T: CloneBacking>(&mut self, values: &'a [T]) -> Result<(), InterpreterError> {
        work(
            self.host,
            HostWorkDimension::VerificationOperations,
            values
                .len()
                .checked_mul(2)
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
        self.allocation(
            values
                .len()
                .checked_mul(size_of::<T>())
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
        if !T::inline() {
            for value in values {
                self.push(value)?;
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), InterpreterError> {
        while let Some(value) = self.pending.pop() {
            value.children(self)?;
        }
        Ok(())
    }
}

pub(crate) fn reserve<T: CloneBacking>(
    value: &T,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        host,
        copy_payload: true,
    };
    walker.push(value)?;
    walker.drain()
}

pub(crate) fn reserve_slice<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        host,
        copy_payload: true,
    };
    walker.slice(values)?;
    walker.drain()
}

pub(crate) fn inspect<T: CloneBacking>(
    value: &T,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        host,
        copy_payload: false,
    };
    walker.push(value)?;
    walker.drain()
}

pub(crate) fn inspect_slice<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let mut walker = Walker {
        pending: Vec::new(),
        capacity: 0,
        host,
        copy_payload: false,
    };
    walker.slice(values)?;
    walker.drain()
}

impl<T: CloneBacking> CloneBacking for Vec<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        walker.slice(self)
    }
}

impl<T: CloneBacking> CloneBacking for Option<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        if let Some(value) = self {
            walker.push(value)?;
        }
        Ok(())
    }
}

impl CloneBacking for String {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        walker.allocation(self.len())
    }
}

impl<K: CloneBacking, V: CloneBacking> CloneBacking for BTreeMap<K, V> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        let (operations, bytes) =
            tree_backing::<K, V>(self.len()).ok_or(InterpreterError::HostWorkRejected)?;
        work(
            walker.host,
            HostWorkDimension::VerificationOperations,
            operations,
        )?;
        walker.allocation(bytes)?;
        for (key, value) in self {
            walker.push(key)?;
            walker.push(value)?;
        }
        Ok(())
    }
}

impl<T> CloneBacking for Arc<T> {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> { Ok(()) }
    fn inline() -> bool { true }
}

impl<K: CloneBacking, V: CloneBacking> CloneBacking for HashMap<K, V> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        let (operations, bytes) =
            hash_backing::<K, V>(self.capacity()).ok_or(InterpreterError::HostWorkRejected)?;
        work(
            walker.host,
            HostWorkDimension::VerificationOperations,
            operations,
        )?;
        walker.allocation(bytes)?;
        for (key, value) in self {
            walker.push(key)?;
            walker.push(value)?;
        }
        Ok(())
    }
}

impl CloneBacking for rspace_plus_plus::rspace::merger::merging_logic::MergeType {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> {
        match self {
            Self::IntegerAdd | Self::BitmaskOr => Ok(()),
        }
    }
}

impl<const N: usize> CloneBacking for [u8; N] {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> { Ok(()) }
    fn inline() -> bool { true }
}

impl<K: CloneBacking> CloneBacking for ResourceMultiset<K> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        walker.push(&self.0)
    }
}

impl<A: CloneBacking, B: CloneBacking> CloneBacking for (A, B) {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
        walker.push(&self.0)?;
        walker.push(&self.1)
    }
}

macro_rules! inline {
    ($($ty:ty),+ $(,)?) => { $(impl CloneBacking for $ty {
        fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> { Ok(()) }
        fn inline() -> bool { true }
    })+ };
}

inline!(
    bool,
    u8,
    i32,
    u32,
    i64,
    u64,
    crate::rust::interpreter::accounting::authority::AuthorityByteEventKind
);

macro_rules! fields {
    ($ty:ident { $($field:ident),* $(,)? }) => {
        impl CloneBacking for $ty {
            fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
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
            fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), InterpreterError> {
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
fields!(AuthorityRuntimeEvent {
    authority,
    debit,
    byte_observation
});
fields!(AuthorityStackBirth {
    produce_hash,
    cells
});

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
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> {
        let Self {} = self;
        Ok(())
    }
}

impl CloneBacking for var::WildcardMsg {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), InterpreterError> {
        let Self {} = self;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
