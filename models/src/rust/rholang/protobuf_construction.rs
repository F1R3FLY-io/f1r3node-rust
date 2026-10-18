use super::protobuf_decoder::owned_slots;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{self};
use crate::rust::par_bag::ParBagComponent;
use crate::rust::par_construction::{
    ParConstruction, ParConstructionError, PlannedComponent, PlannedPar, ProtobufParBuilder,
};

type Result<T> = std::result::Result<T, ParConstructionError>;

#[allow(deprecated)]
pub(crate) fn construction_error(error: ParConstructionError) -> prost::DecodeError {
    prost::DecodeError::new(format!("protobuf construction: {error}"))
}

pub(crate) struct PublicProjection<'a> {
    pub(crate) arena: &'a mut ParConstruction,
}

impl From<rhoapi::Var> for owned_slots::Var {
    fn from(value: rhoapi::Var) -> Self {
        use owned_slots::VarVarInstance as Owned;
        use rhoapi::var::VarInstance;
        Self {
            var_instance: value.var_instance.map(|variant| match variant {
                VarInstance::BoundVar(value) => Owned::BoundVar(value),
                VarInstance::FreeVar(value) => Owned::FreeVar(value),
                VarInstance::Wildcard(rhoapi::var::WildcardMsg {}) => {
                    Owned::Wildcard(owned_slots::WildcardMsg {})
                }
            }),
        }
    }
}

impl owned_slots::Projection for PublicProjection<'_> {
    type Error = prost::DecodeError;

    fn par(&mut self, value: ProtobufParBuilder) -> std::result::Result<rhoapi::Par, Self::Error> {
        let value = self
            .arena
            .freeze_protobuf_par(value)
            .map_err(construction_error)?;
        self.arena.finish(value).map_err(construction_error)
    }

    fn epathmap(
        &mut self,
        value: owned_slots::EPathMap,
    ) -> std::result::Result<rhoapi::EPathMap, Self::Error> {
        let repr = self
            .arena
            .finish_epm_repr(value.repr)
            .map_err(construction_error)?;
        let mut result = rhoapi::EPathMap::default();
        result.locally_free = value.locally_free;
        result.connective_used = value.connective_used;
        result.remainder = value.remainder;
        result.replace_owned_decoded_representation_with(repr, false)?;
        Ok(result)
    }
}

impl owned_slots::EPathMap {
    fn insert_legacy_key(&mut self, key: &[u8]) {
        use crate::rust::epathmap_trie_codec::EPathMapRepr;
        if matches!(self.repr.as_repr(), EPathMapRepr::Empty) {
            *self.repr.as_repr_mut() = EPathMapRepr::Set(pathmap::PathMap::new());
        }
        let EPathMapRepr::Set(map) = self.repr.as_repr_mut() else {
            unreachable!("legacy ingress rejects map mode before insertion")
        };
        crate::rust::rhoapi_ext::insert_epm_set_key(map, key);
    }

    #[allow(deprecated)]
    pub(crate) fn insert_protobuf_par(
        &mut self,
        arena: &mut ParConstruction,
        child: ProtobufParBuilder,
    ) -> std::result::Result<(), prost::DecodeError> {
        if self.repr.as_repr().mode() == crate::rust::epathmap_trie_codec::EPathMapMode::Map {
            return Err(prost::DecodeError::new("EPathMap ps: EPathMap is in Map mode; Set operation would mix set and map membership"));
        }
        let child = arena
            .freeze_protobuf_par(child)
            .map_err(construction_error)?;
        let key = arena
            .canonical_path_bytes(child)
            .map_err(construction_error)?;
        self.insert_legacy_key(&key);
        Ok(())
    }

    #[allow(deprecated)]
    pub(crate) fn merge_serialized_paths(
        &mut self,
        region: &[u8],
    ) -> std::result::Result<(), prost::DecodeError> {
        use crate::rust::epathmap_trie_codec::{EPathMapMode, EPathMapRepr, EpmReprOwner};
        use crate::rust::rhoapi_ext::{join_epm_set_keys, legacy_path_frames};

        if self.repr.as_repr().mode() == EPathMapMode::Map {
            return Err(prost::DecodeError::new(
                "EPathMap serialized_paths cannot be merged into map-mode storage",
            ));
        }
        let mut staged = Self::default();
        for frame in legacy_path_frames(region) {
            let (key, _) =
                crate::rust::canonical_path::prepare_legacy_trie_path(frame?).map_err(|error| {
                    prost::DecodeError::new(format!("EPathMap serialized_paths key: {error:?}"))
                })?;
            staged.insert_legacy_key(&key);
        }
        match (self.repr.as_repr(), staged.repr.as_repr()) {
            (_, EPathMapRepr::Empty) => {}
            (EPathMapRepr::Empty, _) => self.repr = staged.repr,
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {
                let joined = join_epm_set_keys(left, right);
                self.repr = EpmReprOwner::new(EPathMapRepr::Set(joined), Default::default());
            }
            _ => unreachable!("legacy ingress contains only Set or neutral Empty"),
        }
        Ok(())
    }

    pub(crate) fn replace_decoded_representation(
        &mut self,
        incoming: crate::rust::par_construction::OwnedPlannedEPathMap,
        validate: bool,
    ) -> std::result::Result<(), prost::DecodeError> {
        use pathmap::zipper::{Zipper, ZipperIteration, ZipperMoving};

        use crate::rust::epathmap_trie_codec::EPathMapRepr;
        use crate::rust::rhoapi_ext::{canonical_key_metadata, validate_snapshot_mode};

        fn keys<V: Clone + Send + Sync + Unpin + 'static>(
            map: &pathmap::PathMap<V>,
            kind: &str,
            validate: bool,
        ) -> std::result::Result<(), prost::DecodeError> {
            let mut zipper = map.read_zipper();
            if zipper.is_val() {
                canonical_key_metadata(kind, zipper.path(), validate)?;
            }
            while zipper.to_next_val() {
                canonical_key_metadata(kind, zipper.path(), validate)?;
            }
            Ok(())
        }

        validate_snapshot_mode(self.repr.as_repr().mode(), incoming.as_repr().mode())?;
        match incoming.as_repr() {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(map) => keys(map, "set", validate)?,
            EPathMapRepr::Map(map) => keys(map, "map", validate)?,
        }
        self.repr = incoming;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/support/epm_snapshot_admission.rs"]
mod snapshot_admission_tests;

#[cfg(test)]
#[path = "../../../tests/support/epm_legacy_ingress.rs"]
mod legacy_ingress_tests;

pub(crate) struct PlanningProjection<'a> {
    arena: &'a mut ParConstruction,
    children: Vec<PlannedPar>,
    pathmap: Option<owned_slots::EPathMap>,
}

impl owned_slots::Projection for PlanningProjection<'_> {
    type Error = ParConstructionError;

    fn par(&mut self, value: ProtobufParBuilder) -> Result<rhoapi::Par> {
        self.children.push(self.arena.freeze_protobuf_par(value)?);
        Ok(rhoapi::Par::default())
    }

    fn epathmap(&mut self, value: owned_slots::EPathMap) -> Result<rhoapi::EPathMap> {
        assert!(
            self.pathmap.is_none(),
            "one component has at most one immediate EPathMap cut"
        );
        self.pathmap = Some(value);
        Ok(rhoapi::EPathMap::default())
    }
}

pub(crate) fn plan<T: ParBagComponent>(
    arena: &mut ParConstruction,
    project: impl FnOnce(&mut PlanningProjection<'_>) -> Result<T>,
    canonical: impl FnOnce(
        &mut ParConstruction,
        &T,
        Vec<PlannedPar>,
        Option<owned_slots::EPathMap>,
    ) -> Result<PlannedComponent>,
) -> Result<PlannedComponent> {
    let mut projection = PlanningProjection {
        arena,
        children: Vec::new(),
        pathmap: None,
    };
    let shell = project(&mut projection)?;
    if let Some(component) = projection
        .arena
        .incomplete_component(&shell, &projection.children)?
    {
        assert!(
            projection.pathmap.is_none(),
            "an incomplete component has no EPathMap cut"
        );
        return Ok(component);
    }
    canonical(
        projection.arena,
        &shell,
        projection.children,
        projection.pathmap,
    )
}

macro_rules! ordinary {
    ($($name:ident, $ty:ident, $method:ident);+ $(;)?) => {$(
        pub(crate) fn $name(
            arena: &mut ParConstruction,
            shell: &rhoapi::$ty,
            children: Vec<PlannedPar>,
            pathmap: Option<owned_slots::EPathMap>,
        ) -> Result<PlannedComponent> {
            assert!(pathmap.is_none(), "this component has no immediate EPathMap cut");
            arena.$method(shell, &children)
        }
    )+};
}

ordinary! {
    send, Send, send_component;
    new, New, new_component;
    bundle, Bundle, bundle_component;
    conditional, If, if_component;
    connective, Connective, connective_component;
    cost_token_stack, CostTokenStack, cost_token_stack_component;
}

pub(crate) fn unforgeable(
    arena: &mut ParConstruction,
    shell: &rhoapi::GUnforgeable,
    children: Vec<PlannedPar>,
    pathmap: Option<owned_slots::EPathMap>,
) -> Result<PlannedComponent> {
    assert!(
        children.is_empty() && pathmap.is_none(),
        "an unforgeable has no recursive cuts"
    );
    arena.unforgeable_component(shell)
}

pub(crate) fn receive(
    arena: &mut ParConstruction,
    shell: &rhoapi::Receive,
    mut children: Vec<PlannedPar>,
    pathmap: Option<owned_slots::EPathMap>,
) -> Result<PlannedComponent> {
    assert!(pathmap.is_none(), "Receive has no immediate EPathMap cut");
    if shell.condition.is_none() {
        children.push(arena.empty_par()?);
    }
    arena.receive_component(shell, &children)
}

pub(crate) fn matching(
    arena: &mut ParConstruction,
    shell: &rhoapi::Match,
    children: Vec<PlannedPar>,
    pathmap: Option<owned_slots::EPathMap>,
) -> Result<PlannedComponent> {
    assert!(pathmap.is_none(), "Match has no immediate EPathMap cut");
    let mut wire = children.into_iter();
    let mut semantic = Vec::new();
    semantic.push(wire.next().expect("a complete Match has a target"));
    for case in &shell.cases {
        semantic.push(wire.next().expect("a complete MatchCase has a pattern"));
        semantic.push(wire.next().expect("a complete MatchCase has a source"));
        semantic.push(if case.guard.is_some() {
            wire.next()
                .expect("a present MatchCase guard has one planned child")
        } else {
            arena.empty_par()?
        });
    }
    assert!(
        wire.next().is_none(),
        "every Match protobuf child was consumed"
    );
    arena.match_component(shell, &semantic)
}

pub(crate) fn expression(
    arena: &mut ParConstruction,
    shell: &rhoapi::Expr,
    mut children: Vec<PlannedPar>,
    pathmap: Option<owned_slots::EPathMap>,
) -> Result<PlannedComponent> {
    if let Some(pathmap) = pathmap {
        assert!(
            children.is_empty(),
            "an EPathMap cut owns its planned values"
        );
        return match shell.expr_instance.as_ref() {
            Some(ExprInstance::EPathmapBody(_)) => arena.owned_epathmap_repr_expr_component(
                pathmap.repr,
                pathmap.locally_free,
                pathmap.connective_used,
                pathmap.remainder,
            ),
            Some(ExprInstance::EZipperBody(zipper)) => arena.owned_ezipper_repr_expr_component(
                zipper,
                pathmap.repr,
                pathmap.locally_free,
                pathmap.connective_used,
                pathmap.remainder,
            ),
            _ => {
                unreachable!("only EPathMap and EZipper expressions own an immediate EPathMap cut")
            }
        };
    }
    if matches!(shell.expr_instance, Some(ExprInstance::EMethodBody(_))) {
        children.rotate_left(1);
    }
    arena.expr_component(shell, &children, false)
}
