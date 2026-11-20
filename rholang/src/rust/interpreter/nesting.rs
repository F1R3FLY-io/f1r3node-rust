use std::fmt;

use serde::ser::{self, Serialize};
pub use shared::rust::stack::{MAX_EVALUATION_DEPTH, MAX_NESTING_DEPTH};

use super::errors::InterpreterError;

/// Fails when `value` nests `Par`s deeper than [`MAX_NESTING_DEPTH`]. The walk
/// follows the derived `Serialize` implementations, so it reaches every field,
/// and it stops as soon as the limit is passed, so a deeper value cannot make
/// it recurse further.
pub fn check_nesting<T: Serialize + ?Sized>(value: &T) -> Result<(), InterpreterError> {
    let mut probe = DepthProbe {
        depth: 0,
        limit: MAX_NESTING_DEPTH,
    };
    value
        .serialize(&mut probe)
        .map_err(|TooDeep| InterpreterError::NestingLimitExceeded {
            limit: MAX_NESTING_DEPTH,
        })
}

const PAR: &str = "Par";

struct DepthProbe {
    depth: usize,
    limit: usize,
}

#[derive(Debug)]
struct TooDeep;

impl fmt::Display for TooDeep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("nesting limit exceeded")
    }
}

impl std::error::Error for TooDeep {}

impl ser::Error for TooDeep {
    fn custom<T: fmt::Display>(_msg: T) -> Self { TooDeep }
}

impl DepthProbe {
    fn enter(&mut self, name: &'static str) -> Result<bool, TooDeep> {
        if name != PAR {
            return Ok(false);
        }
        self.depth += 1;
        if self.depth > self.limit {
            Err(TooDeep)
        } else {
            Ok(true)
        }
    }
}

/// The compound serializer for one nested value: leaves the `Par` it entered,
/// if it entered one, when the value ends.
struct Compound<'p> {
    probe: &'p mut DepthProbe,
    entered_par: bool,
}

impl Compound<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        value.serialize(&mut *self.probe)
    }

    fn finish(self) -> Result<(), TooDeep> {
        if self.entered_par {
            self.probe.depth -= 1;
        }
        Ok(())
    }
}

impl<'p> ser::Serializer for &'p mut DepthProbe {
    type Ok = ();
    type Error = TooDeep;
    type SerializeSeq = Compound<'p>;
    type SerializeTuple = Compound<'p>;
    type SerializeTupleStruct = Compound<'p>;
    type SerializeTupleVariant = Compound<'p>;
    type SerializeMap = Compound<'p>;
    type SerializeStruct = Compound<'p>;
    type SerializeStructVariant = Compound<'p>;

    fn serialize_bool(self, _: bool) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_i8(self, _: i8) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_i16(self, _: i16) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_i32(self, _: i32) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_i64(self, _: i64) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_i128(self, _: i128) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_u8(self, _: u8) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_u16(self, _: u16) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_u32(self, _: u32) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_u64(self, _: u64) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_u128(self, _: u128) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_f32(self, _: f32) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_f64(self, _: f64) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_char(self, _: char) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_str(self, _: &str) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_bytes(self, _: &[u8]) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_none(self) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), TooDeep> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_unit_struct(self, _: &'static str) -> Result<(), TooDeep> { Ok(()) }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), TooDeep> {
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), TooDeep> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), TooDeep> {
        value.serialize(self)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }

    fn serialize_tuple(self, _: usize) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }

    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Compound<'p>, TooDeep> {
        let entered_par = self.enter(name)?;
        Ok(Compound {
            probe: self,
            entered_par,
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Compound<'p>, TooDeep> {
        Ok(Compound {
            probe: self,
            entered_par: false,
        })
    }
}

impl ser::SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), TooDeep> {
        self.element(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}

impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = TooDeep;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), TooDeep> {
        self.element(value)
    }

    fn end(self) -> Result<(), TooDeep> { self.finish() }
}
