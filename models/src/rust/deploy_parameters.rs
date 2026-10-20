use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::casper;
use crate::casper::rholang_value::Value;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{EList, ETuple, Expr, Par};
use crate::rust::par_map::ParMap;
use crate::rust::par_map_type_mapper::ParMapTypeMapper;
use crate::rust::par_set::ParSet;
use crate::rust::par_set_type_mapper::ParSetTypeMapper;

pub const MAX_PARAMETERS: usize = 50;
pub const MAX_PARAMETER_NAME_BYTES: usize = 256;
pub const MAX_PARAMETER_DEPTH: usize = 32;
pub const MAX_PARAMETER_NODES: usize = 10_000;
pub const MAX_PARAMETER_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeployParameter {
    pub name: String,
    pub value: RholangValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RholangValue {
    Bool(bool),
    Int(i64),
    String(String),
    Bytes(Vec<u8>),
    #[schema(no_recursion)]
    Tuple(Vec<RholangValue>),
    #[schema(no_recursion)]
    List(Vec<RholangValue>),
    #[schema(no_recursion)]
    Set(Vec<RholangValue>),
    Map(Vec<DeployMapEntry>),
    Nil,
    Uri(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeployMapEntry {
    #[schema(no_recursion)]
    pub key: RholangValue,
    #[schema(no_recursion)]
    pub value: RholangValue,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParameterError {
    #[error("The deploy has more than {MAX_PARAMETERS} parameters.")]
    TooManyParameters,
    #[error("Use 1 to {MAX_PARAMETER_NAME_BYTES} ASCII letters, digits, or underscores. Start the name with a letter or underscore.")]
    InvalidName,
    #[error("Parameter names must be unique.")]
    DuplicateName,
    #[error("A parameter value is missing.")]
    MissingValue,
    #[error("Parameter nesting exceeds {MAX_PARAMETER_DEPTH} levels.")]
    TooDeep,
    #[error("Parameters contain more than {MAX_PARAMETER_NODES} values.")]
    TooManyValues,
    #[error("Parameter data exceeds {MAX_PARAMETER_BYTES} bytes.")]
    TooLarge,
    #[error("Map keys must be unique Rholang values.")]
    DuplicateMapKey,
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}

impl Budget {
    fn enter(&mut self, depth: usize) -> Result<(), ParameterError> {
        if depth > MAX_PARAMETER_DEPTH {
            return Err(ParameterError::TooDeep);
        }
        self.nodes += 1;
        if self.nodes > MAX_PARAMETER_NODES {
            return Err(ParameterError::TooManyValues);
        }
        self.add_bytes(8)
    }

    fn add_bytes(&mut self, count: usize) -> Result<(), ParameterError> {
        self.bytes = self
            .bytes
            .checked_add(count)
            .ok_or(ParameterError::TooLarge)?;
        if self.bytes > MAX_PARAMETER_BYTES {
            return Err(ParameterError::TooLarge);
        }
        Ok(())
    }
}

pub fn validate_parameters(parameters: &[DeployParameter]) -> Result<(), ParameterError> {
    prepare_parameters::<false>(parameters).map(|_| ())
}

pub(crate) fn parameters_to_par(
    parameters: &[DeployParameter],
) -> Result<Vec<Par>, ParameterError> {
    prepare_parameters::<true>(parameters)
}

fn prepare_parameters<const BUILD_PAR: bool>(
    parameters: &[DeployParameter],
) -> Result<Vec<Par>, ParameterError> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(ParameterError::TooManyParameters);
    }
    let mut names = HashSet::new();
    let mut budget = Budget::default();
    let mut pars = Vec::new();
    for parameter in parameters {
        if parameter.name.is_empty()
            || parameter.name.len() > MAX_PARAMETER_NAME_BYTES
            || !parameter
                .name
                .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            || !parameter
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(ParameterError::InvalidName);
        }
        if !names.insert(&parameter.name) {
            return Err(ParameterError::DuplicateName);
        }
        budget.add_bytes(parameter.name.len())?;
        let par = parameter.value.prepare::<BUILD_PAR>(1, &mut budget)?;
        if BUILD_PAR {
            pars.push(par);
        }
    }
    Ok(pars)
}

pub fn deserialize_parameters<'de, D>(deserializer: D) -> Result<Vec<DeployParameter>, D::Error>
where D: serde::Deserializer<'de> {
    let parameters = Vec::<DeployParameter>::deserialize(deserializer)?;
    validate_parameters(&parameters).map_err(serde::de::Error::custom)?;
    Ok(parameters)
}

pub fn parameters_from_proto(
    parameters: Vec<casper::DeployParameter>,
) -> Result<Vec<DeployParameter>, ParameterError> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(ParameterError::TooManyParameters);
    }
    let mut budget = Budget::default();
    let result = parameters
        .into_iter()
        .map(|parameter| {
            budget.add_bytes(parameter.name.len())?;
            Ok(DeployParameter {
                name: parameter.name,
                value: RholangValue::from_proto(
                    parameter.value.ok_or(ParameterError::MissingValue)?,
                    1,
                    &mut budget,
                )?,
            })
        })
        .collect::<Result<Vec<_>, ParameterError>>()?;
    validate_parameters(&result)?;
    Ok(result)
}

impl DeployParameter {
    pub fn to_proto(&self) -> casper::DeployParameter {
        casper::DeployParameter {
            name: self.name.clone(),
            value: Some(self.value.to_proto()),
        }
    }
}

impl RholangValue {
    fn prepare<const BUILD_PAR: bool>(
        &self,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Par, ParameterError> {
        budget.enter(depth)?;
        match self {
            Self::String(value) | Self::Uri(value) => budget.add_bytes(value.len())?,
            Self::Bytes(value) => budget.add_bytes(value.len())?,
            Self::Tuple(values) | Self::List(values) | Self::Set(values) => {
                let mut pars = Vec::new();
                for value in values {
                    let par = value.prepare::<BUILD_PAR>(depth + 1, budget)?;
                    if BUILD_PAR {
                        pars.push(par);
                    }
                }
                return Ok(if BUILD_PAR {
                    self.sequence_to_par(pars)
                } else {
                    Par::default()
                });
            }
            Self::Map(entries) => {
                let mut keys = HashSet::new();
                let mut pairs = HashMap::new();
                for entry in entries {
                    let key = entry.key.prepare::<true>(depth + 1, budget)?;
                    let value = entry.value.prepare::<BUILD_PAR>(depth + 1, budget)?;
                    let duplicate = if BUILD_PAR {
                        pairs.insert(key, value).is_some()
                    } else {
                        !keys.insert(key)
                    };
                    if duplicate {
                        return Err(ParameterError::DuplicateMapKey);
                    }
                }
                return Ok(if BUILD_PAR {
                    Self::map_to_par(pairs.into_iter().collect())
                } else {
                    Par::default()
                });
            }
            Self::Bool(_) | Self::Int(_) | Self::Nil => {}
        }
        Ok(if BUILD_PAR {
            self.to_par()
        } else {
            Par::default()
        })
    }

    fn sequence_to_par(&self, ps: Vec<Par>) -> Par {
        let expr_instance = match self {
            Self::Tuple(_) => ExprInstance::ETupleBody(ETuple {
                ps,
                ..Default::default()
            }),
            Self::List(_) => ExprInstance::EListBody(EList {
                ps,
                ..Default::default()
            }),
            Self::Set(_) => ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(
                ParSet::create_from_vec(ps),
            )),
            _ => unreachable!(),
        };
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(expr_instance),
        }])
    }

    fn map_to_par(pairs: Vec<(Par, Par)>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
                ParMap::create_from_vec(pairs),
            ))),
        }])
    }

    pub fn to_par(&self) -> Par {
        let expr_instance = match self {
            Self::Bool(value) => ExprInstance::GBool(*value),
            Self::Int(value) => ExprInstance::GInt(*value),
            Self::String(value) => ExprInstance::GString(value.clone()),
            Self::Bytes(value) => ExprInstance::GByteArray(value.clone()),
            Self::Uri(value) => ExprInstance::GUri(value.clone()),
            Self::Nil => return Par::default(),
            Self::Tuple(values) | Self::List(values) | Self::Set(values) => {
                return self.sequence_to_par(values.iter().map(Self::to_par).collect());
            }
            Self::Map(entries) => {
                return Self::map_to_par(
                    entries
                        .iter()
                        .map(|entry| (entry.key.to_par(), entry.value.to_par()))
                        .collect(),
                );
            }
        };
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(expr_instance),
        }])
    }

    pub fn to_proto(&self) -> casper::RholangValue {
        let sequence = |values: &[Self]| casper::RholangSequence {
            values: values.iter().map(Self::to_proto).collect(),
        };
        let value = match self {
            Self::Bool(value) => Value::BoolValue(*value),
            Self::Int(value) => Value::IntValue(*value),
            Self::String(value) => Value::StringValue(value.clone()),
            Self::Bytes(value) => Value::BytesValue(value.clone().into()),
            Self::Tuple(values) => Value::TupleValue(sequence(values)),
            Self::List(values) => Value::ListValue(sequence(values)),
            Self::Set(values) => Value::SetValue(sequence(values)),
            Self::Map(entries) => Value::MapValue(casper::RholangMap {
                entries: entries
                    .iter()
                    .map(|entry| casper::RholangMapEntry {
                        key: Some(entry.key.to_proto()),
                        value: Some(entry.value.to_proto()),
                    })
                    .collect(),
            }),
            Self::Nil => Value::NilValue(casper::RholangNil {}),
            Self::Uri(value) => Value::UriValue(value.clone()),
        };
        casper::RholangValue { value: Some(value) }
    }

    fn from_proto(
        proto: casper::RholangValue,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Self, ParameterError> {
        budget.enter(depth)?;
        let mut sequence = |values: Vec<casper::RholangValue>| {
            values
                .into_iter()
                .map(|value| Self::from_proto(value, depth + 1, budget))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match proto.value.ok_or(ParameterError::MissingValue)? {
            Value::BoolValue(value) => Self::Bool(value),
            Value::IntValue(value) => Self::Int(value),
            Value::StringValue(value) => {
                budget.add_bytes(value.len())?;
                Self::String(value)
            }
            Value::BytesValue(value) => {
                budget.add_bytes(value.len())?;
                Self::Bytes(value.to_vec())
            }
            Value::UriValue(value) => {
                budget.add_bytes(value.len())?;
                Self::Uri(value)
            }
            Value::TupleValue(value) => Self::Tuple(sequence(value.values)?),
            Value::ListValue(value) => Self::List(sequence(value.values)?),
            Value::SetValue(value) => Self::Set(sequence(value.values)?),
            Value::MapValue(value) => Self::Map(
                value
                    .entries
                    .into_iter()
                    .map(|entry| {
                        Ok(DeployMapEntry {
                            key: Self::from_proto(
                                entry.key.ok_or(ParameterError::MissingValue)?,
                                depth + 1,
                                budget,
                            )?,
                            value: Self::from_proto(
                                entry.value.ok_or(ParameterError::MissingValue)?,
                                depth + 1,
                                budget,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, ParameterError>>()?,
            ),
            Value::NilValue(_) => Self::Nil,
        })
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use prost::Message;

    use super::*;
    use crate::rust::casper::protocol::casper_message::DeployData;
    use crate::rust::normalizer_env::normalizer_env_from_deploy;

    fn parameter(value: RholangValue) -> DeployParameter {
        DeployParameter {
            name: "input".into(),
            value,
        }
    }

    fn check_parameters(parameters: &[DeployParameter]) -> Result<(), ParameterError> {
        let result = validate_parameters(parameters);
        assert_eq!(parameters_to_par(parameters).map(|_| ()), result);
        result
    }

    fn deploy(parameters: Vec<DeployParameter>) -> DeployData {
        DeployData {
            term: "Nil".into(),
            time_stamp: 1,
            phlo_price: 2,
            phlo_limit: 3,
            valid_after_block_number: 4,
            shard_id: "root".into(),
            expiration_timestamp: Some(500),
            parameters,
        }
    }

    fn values() -> Vec<RholangValue> {
        vec![
            RholangValue::Bool(false),
            RholangValue::Bool(true),
            RholangValue::Int(i64::MIN),
            RholangValue::Int(0),
            RholangValue::Int(i64::MAX),
            RholangValue::String("text\0λ".into()),
            RholangValue::String(String::new()),
            RholangValue::Bytes(vec![0, 127, 255]),
            RholangValue::Bytes(Vec::new()),
            RholangValue::Tuple(vec![RholangValue::Int(1), RholangValue::Nil]),
            RholangValue::List(vec![
                RholangValue::Bool(true),
                RholangValue::String("x".into()),
            ]),
            RholangValue::Set(vec![RholangValue::Int(2), RholangValue::Int(1)]),
            RholangValue::Map(vec![DeployMapEntry {
                key: RholangValue::Tuple(vec![RholangValue::Int(1)]),
                value: RholangValue::List(vec![RholangValue::Bytes(vec![0]), RholangValue::Nil]),
            }]),
            RholangValue::Tuple(Vec::new()),
            RholangValue::List(Vec::new()),
            RholangValue::Set(Vec::new()),
            RholangValue::Map(Vec::new()),
            RholangValue::Nil,
            RholangValue::Uri("rho:io:stdout".into()),
        ]
    }

    #[test]
    fn deploy_parameters_preserve_all_types_through_json_proto_and_signatures() {
        for value in values() {
            let original = deploy(vec![parameter(value)]);
            let json = serde_json::to_string(&original).unwrap();
            assert_eq!(serde_json::from_str::<DeployData>(&json).unwrap(), original);
            assert_eq!(
                DeployData::decode(DeployData::encode(original.clone())).unwrap(),
                original
            );
            let signed = Signed::create(
                original,
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[1; 32]),
            )
            .unwrap();
            let proto = casper::DeployDataProto::decode(
                DeployData::to_proto_ref(&signed).encode_to_vec().as_slice(),
            )
            .unwrap();
            let restored = DeployData::from_proto(proto.clone()).unwrap();
            assert_eq!(restored, signed);
            assert_eq!(
                normalizer_env_from_deploy(&restored).unwrap()["rho:deploy:param:input"],
                signed.data.parameters[0].value.to_par()
            );
            let mut tampered = proto;
            tampered.parameters[0].name = "other".into();
            assert!(DeployData::from_proto(tampered).is_err());
            let mut tampered = DeployData::to_proto_ref(&signed);
            tampered.parameters.clear();
            assert!(DeployData::from_proto(tampered).is_err());
            let mut tampered = DeployData::to_proto_ref(&signed);
            tampered.parameters[0].value = Some(RholangValue::Int(42).to_proto());
            assert!(DeployData::from_proto(tampered).is_err());
        }
    }

    #[test]
    fn deploy_parameters_preserve_legacy_scalar_wire_values() {
        for (value, bytes) in [
            (RholangValue::Bool(false), vec![0x08, 0x00]),
            (RholangValue::Bool(true), vec![0x08, 0x01]),
            (RholangValue::Int(-1), vec![0x10, 0x01]),
            (RholangValue::Int(1), vec![0x10, 0x02]),
            (RholangValue::String("x".into()), vec![0x1a, 0x01, b'x']),
            (RholangValue::Bytes(vec![255]), vec![0x22, 0x01, 255]),
        ] {
            assert_eq!(value.to_proto().encode_to_vec(), bytes);
            let proto = casper::RholangValue::decode(bytes.as_slice()).unwrap();
            assert_eq!(
                parameters_from_proto(vec![casper::DeployParameter {
                    name: "input".into(),
                    value: Some(proto),
                }])
                .unwrap(),
                vec![parameter(value)]
            );
        }
    }

    #[test]
    fn deploy_parameters_empty_preserves_legacy_wire_bytes_and_json() {
        let original = deploy(Vec::new());
        let bytes = hex::decode("12034e696c18013802400350045a04726f6f7468f403").unwrap();
        assert_eq!(DeployData::encode(original.clone()), bytes);
        assert_eq!(DeployData::decode(bytes).unwrap(), original);
        let json = serde_json::to_value(&original).unwrap();
        assert!(json.get("parameters").is_none());
        assert_eq!(
            serde_json::from_value::<DeployData>(json).unwrap(),
            original
        );
    }

    #[test]
    fn deploy_parameters_collections_have_rholang_order_and_identity() {
        let forward = RholangValue::Set(vec![RholangValue::Int(1), RholangValue::Int(2)]);
        let reverse = RholangValue::Set(vec![
            RholangValue::Int(2),
            RholangValue::Int(1),
            RholangValue::Int(1),
        ]);
        assert_eq!(
            forward.to_par().encode_to_vec(),
            reverse.to_par().encode_to_vec()
        );
        let entries = vec![
            DeployMapEntry {
                key: RholangValue::Int(1),
                value: RholangValue::Nil,
            },
            DeployMapEntry {
                key: RholangValue::String("x".into()),
                value: reverse.clone(),
            },
        ];
        assert_eq!(
            RholangValue::Map(entries.clone()).to_par().encode_to_vec(),
            RholangValue::Map(entries.into_iter().rev().collect())
                .to_par()
                .encode_to_vec()
        );
        assert_ne!(
            RholangValue::List(vec![RholangValue::Int(1), RholangValue::Int(2)]).to_par(),
            RholangValue::List(vec![RholangValue::Int(2), RholangValue::Int(1)]).to_par()
        );
        let duplicate = parameter(RholangValue::Map(vec![
            DeployMapEntry {
                key: forward,
                value: RholangValue::Int(1),
            },
            DeployMapEntry {
                key: reverse,
                value: RholangValue::Int(2),
            },
        ]));
        assert_eq!(
            check_parameters(&[duplicate]),
            Err(ParameterError::DuplicateMapKey)
        );
    }

    #[test]
    fn deploy_parameters_prepare_nested_maps_in_parameter_order() {
        let entries = vec![
            DeployMapEntry {
                key: RholangValue::Set(vec![RholangValue::Int(2), RholangValue::Int(1)]),
                value: RholangValue::Nil,
            },
            DeployMapEntry {
                key: RholangValue::Int(3),
                value: RholangValue::Tuple(vec![RholangValue::Bool(true)]),
            },
        ];
        let forward = RholangValue::Map(entries.clone());
        let reverse = RholangValue::Map(entries.into_iter().rev().collect());
        let mut values = values();
        values.push(RholangValue::Map(vec![DeployMapEntry {
            key: forward.clone(),
            value: RholangValue::List(vec![reverse.clone()]),
        }]));
        let parameters: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| DeployParameter {
                name: format!("input{index}"),
                value,
            })
            .collect();
        assert!(check_parameters(&parameters).is_ok());
        let prepared = parameters_to_par(&parameters).unwrap();
        assert_eq!(prepared.len(), parameters.len());
        for (par, parameter) in prepared.iter().zip(&parameters) {
            assert_eq!(
                par.encode_to_vec(),
                parameter.value.to_par().encode_to_vec()
            );
        }
        let signed = Signed::create(
            deploy(parameters.clone()),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let env = normalizer_env_from_deploy(&signed).unwrap();
        for (par, parameter) in prepared.iter().zip(&parameters) {
            assert_eq!(&env[&format!("rho:deploy:param:{}", parameter.name)], par);
        }
        let duplicate = RholangValue::Map(vec![
            DeployMapEntry {
                key: forward,
                value: RholangValue::Nil,
            },
            DeployMapEntry {
                key: reverse,
                value: RholangValue::Nil,
            },
        ]);
        for value in [
            duplicate.clone(),
            RholangValue::Map(vec![DeployMapEntry {
                key: duplicate.clone(),
                value: RholangValue::Nil,
            }]),
            RholangValue::Map(vec![DeployMapEntry {
                key: RholangValue::Nil,
                value: duplicate,
            }]),
        ] {
            assert_eq!(
                check_parameters(&[parameter(value)]),
                Err(ParameterError::DuplicateMapKey)
            );
        }
    }

    #[test]
    fn deploy_parameters_reject_invalid_names_and_duplicate_names() {
        for name in [
            "".into(),
            "0input".into(),
            "a-b".into(),
            "a:b".into(),
            "a b".into(),
            "λ".into(),
            "x".repeat(257),
        ] {
            assert_eq!(
                check_parameters(&[DeployParameter {
                    name,
                    value: RholangValue::Nil
                }]),
                Err(ParameterError::InvalidName)
            );
        }
        assert!(check_parameters(&[DeployParameter {
            name: "_A0".into(),
            value: RholangValue::Nil
        }])
        .is_ok());
        let value = parameter(RholangValue::Nil);
        assert_eq!(
            check_parameters(&[value.clone(), value.clone()]),
            Err(ParameterError::DuplicateName)
        );
        assert_eq!(
            check_parameters(&vec![value; MAX_PARAMETERS + 1]),
            Err(ParameterError::TooManyParameters)
        );
    }

    #[test]
    fn deploy_parameters_enforce_depth_node_and_byte_budgets() {
        let mut value = RholangValue::Nil;
        for _ in 1..MAX_PARAMETER_DEPTH {
            value = RholangValue::List(vec![value]);
        }
        assert!(check_parameters(&[parameter(value.clone())]).is_ok());
        let too_deep = parameter(RholangValue::List(vec![value]));
        assert_eq!(
            check_parameters(&[too_deep.clone()]),
            Err(ParameterError::TooDeep)
        );
        assert_eq!(
            parameters_from_proto(vec![too_deep.to_proto()]),
            Err(ParameterError::TooDeep)
        );
        assert_eq!(
            check_parameters(&[parameter(RholangValue::List(vec![
                RholangValue::Nil;
                MAX_PARAMETER_NODES
            ]))]),
            Err(ParameterError::TooManyValues)
        );
        assert_eq!(
            check_parameters(&[parameter(RholangValue::Bytes(vec![0; MAX_PARAMETER_BYTES]))]),
            Err(ParameterError::TooLarge)
        );
        let half = RholangValue::String("x".repeat(MAX_PARAMETER_BYTES / 2));
        assert_eq!(
            check_parameters(&[parameter(RholangValue::Tuple(vec![half.clone(), half]))]),
            Err(ParameterError::TooLarge)
        );
        let at_node_limit = parameter(RholangValue::List(vec![
            RholangValue::Nil;
            MAX_PARAMETER_NODES - 1
        ]));
        assert!(check_parameters(&[at_node_limit.clone()]).is_ok());
        assert_eq!(
            check_parameters(&[at_node_limit, DeployParameter {
                name: "extra".into(),
                value: RholangValue::Nil,
            },]),
            Err(ParameterError::TooManyValues)
        );
        let at_byte_limit = parameter(RholangValue::Bytes(vec![
            0;
            MAX_PARAMETER_BYTES
                - "input".len()
                - 8
        ]));
        assert!(check_parameters(&[at_byte_limit.clone()]).is_ok());
        assert_eq!(
            check_parameters(&[at_byte_limit, DeployParameter {
                name: "extra".into(),
                value: RholangValue::Nil,
            },]),
            Err(ParameterError::TooLarge)
        );
    }

    #[test]
    fn deploy_parameters_preserve_map_validation_error_order() {
        let entry = DeployMapEntry {
            key: RholangValue::Nil,
            value: RholangValue::Nil,
        };
        let oversized = DeployMapEntry {
            key: RholangValue::Nil,
            value: RholangValue::Bytes(vec![0; MAX_PARAMETER_BYTES]),
        };
        assert_eq!(
            check_parameters(&[parameter(RholangValue::Map(vec![
                entry.clone(),
                entry.clone(),
                oversized.clone(),
            ]))]),
            Err(ParameterError::DuplicateMapKey)
        );
        assert_eq!(
            check_parameters(&[parameter(RholangValue::Map(vec![entry, oversized]))]),
            Err(ParameterError::TooLarge)
        );
    }

    #[test]
    fn deploy_parameters_reject_missing_proto_values_and_malformed_json() {
        for value in [
            None,
            Some(casper::RholangValue { value: None }),
            Some(casper::RholangValue {
                value: Some(Value::MapValue(casper::RholangMap {
                    entries: vec![casper::RholangMapEntry {
                        key: None,
                        value: Some(RholangValue::Nil.to_proto()),
                    }],
                })),
            }),
        ] {
            assert_eq!(
                parameters_from_proto(vec![casper::DeployParameter {
                    name: "input".into(),
                    value
                }]),
                Err(ParameterError::MissingValue)
            );
        }
        for value in [
            serde_json::json!({"type":"int","value":1.5}),
            serde_json::json!({"type":"int","value":9223372036854775808u64}),
            serde_json::json!({"type":"bytes","value":[256]}),
            serde_json::json!({"type":"unknown","value":1}),
            serde_json::json!({"type":"bool","value":1}),
        ] {
            let mut json = serde_json::to_value(deploy(Vec::new())).unwrap();
            json["parameters"] = serde_json::json!([{"name":"input","value":value}]);
            assert!(serde_json::from_value::<DeployData>(json).is_err());
        }
        let mut json = serde_json::to_value(deploy(Vec::new())).unwrap();
        json["parameters"] = serde_json::json!([
            {"name":"input","value":{"type":"nil"}},
            {"name":"input","value":{"type":"nil"}}
        ]);
        assert!(serde_json::from_value::<DeployData>(json).is_err());
    }

    #[test]
    fn deploy_parameters_openapi_schema_handles_recursive_values() {
        #[derive(utoipa::OpenApi)]
        #[openapi(components(schemas(DeployData)))]
        struct Api;
        let schema = <Api as utoipa::OpenApi>::openapi();
        let schemas = &schema.components.unwrap().schemas;
        for name in [
            "DeployData",
            "DeployParameter",
            "RholangValue",
            "DeployMapEntry",
        ] {
            assert!(schemas.contains_key(name));
        }
    }
}
