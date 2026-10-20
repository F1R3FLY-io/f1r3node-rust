pub use super::bincode_decoder::{
    cold_decode_list_bind_patterns_for_test, cold_decode_par_with_random_for_test,
};
pub use super::bincode_schema_tables::*;
pub use super::par_children::{connective_instance_variant_index, expr_instance_variant_index};
pub const EXPR_INSTANCE_VARIANT_COUNT: u32 =
    super::bincode_schema_tables::EXPR_INSTANCE_VARIANT_COUNT as u32;
pub const CONNECTIVE_INSTANCE_VARIANT_COUNT: u32 =
    super::bincode_schema_tables::CONNECTIVE_INSTANCE_VARIANT_COUNT as u32;
pub const COST_VALUE_VARIANT_COUNT: u32 = super::bincode_schema_tables::VALUE_VARIANT_COUNT as u32;
pub const VAR_INSTANCE_VARIANT_COUNT: u32 =
    super::bincode_schema_tables::VAR_INSTANCE_VARIANT_COUNT as u32;
pub const UNF_INSTANCE_VARIANT_COUNT: u32 =
    super::bincode_schema_tables::UNF_INSTANCE_VARIANT_COUNT as u32;
pub const TAGGED_CONT_VARIANT_COUNT: u32 =
    super::bincode_schema_tables::TAGGED_CONT_VARIANT_COUNT as u32;

pub fn cost_value_variant_index(value: &crate::rhoapi::cost_signature::Value) -> u32 {
    use crate::rhoapi::cost_signature::Value;
    match value {
        Value::Ground(_) => CS_GROUND,
        Value::BoundLevel(_) => CS_BOUND_LEVEL,
        Value::Quote(_) => CS_QUOTE,
        Value::Compound(_) => CS_COMPOUND,
        Value::Name(_) => CS_NAME,
        Value::Unit(_) => CS_UNIT,
    }
}
