use std::collections::BTreeMap;

use super::match_schema_tables::{ParCursor, ParSequenceCursor};
use super::protobuf_schema::{ProtobufField, ProtobufKind};
use crate::rhoapi::Par;
use crate::rust::par_bag::ParBag;
use crate::rust::rhoapi_ext::EPathMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhoMatchFieldKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    String,
    StringSequence,
    BytesSequence,
    OptionalMessage(u16),
    MessageSequence(u16),
    StringMessageMap(u16),
    Oneof(&'static [RhoMatchOneofVariant]),
    ParBag,
    PathMap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhoMatchPayloadKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    String,
    Message(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchOneofVariant {
    pub tag: u32,
    pub name: &'static str,
    pub payload: RhoMatchPayloadKind,
    pub protobuf_kind: ProtobufKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchField {
    pub tag: u32,
    pub name: &'static str,
    pub kind: RhoMatchFieldKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchProgram {
    pub type_id: u16,
    pub type_name: &'static str,
    pub fields: &'static [RhoMatchField],
    pub protobuf_fields: &'static [ProtobufField],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RhoScalarRef<'a> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(&'a [u8]),
    String(&'a str),
    StringSequence(&'a [String]),
    BytesSequence(&'a [Vec<u8>]),
}

#[derive(Clone, Copy)]
pub enum RhoOneofPayload<'a> {
    Scalar(RhoScalarRef<'a>),
    Message(ParCursor<'a>),
}

#[derive(Clone, Copy)]
pub struct RhoOneofCursor<'a> {
    pub oneof_name: &'static str,
    pub variant_name: &'static str,
    pub serde_index: u32,
    pub protobuf_tag: u32,
    pub payload: RhoOneofPayload<'a>,
}

#[derive(Clone, Copy)]
pub enum RhoMatchValue<'a> {
    Scalar(RhoScalarRef<'a>),
    OptionalMessage(Option<ParCursor<'a>>),
    MessageSequence(ParSequenceCursor<'a>),
    StringMessageMap(&'a BTreeMap<String, Par>),
    Oneof(Option<RhoOneofCursor<'a>>),
    ParBag(&'a ParBag),
    PathMap(&'a EPathMap),
}

#[derive(Clone, Copy)]
pub enum RhoPathEvent<'a> {
    EnterNode {
        cursor: ParCursor<'a>,
        program: &'static RhoMatchProgram,
    },
    LeaveNode,
    EnterField(&'static RhoMatchField),
    LeaveField,
    Scalar(RhoScalarRef<'a>),
    Absent,
    EnterSequence(usize),
    LeaveSequence,
    EnterMap(usize),
    MapKey(&'a str),
    LeaveMap,
    EnterOneof(RhoOneofCursor<'a>),
    LeaveOneof,
    ParBag(&'a ParBag),
    PathMap(&'a EPathMap),
}

enum RhoPathStep<'a> {
    Emit(RhoPathEvent<'a>),
    Node {
        cursor: ParCursor<'a>,
        field: usize,
    },
    Value(RhoMatchValue<'a>),
    Sequence {
        cursor: ParSequenceCursor<'a>,
        index: usize,
    },
    Map {
        entries: std::collections::btree_map::Iter<'a, String, Par>,
    },
    Oneof(RhoOneofCursor<'a>),
}

pub struct RhoPathCursor<'a> {
    work: Vec<RhoPathStep<'a>>,
}

impl<'a> RhoPathCursor<'a> {
    pub fn new(root: ParCursor<'a>) -> Self {
        Self {
            work: vec![RhoPathStep::Node {
                cursor: root,
                field: 0,
            }],
        }
    }

    pub fn par(root: &'a Par) -> Self { Self::new(ParCursor::Par(root)) }
}

impl<'a> Iterator for RhoPathCursor<'a> {
    type Item = RhoPathEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                RhoPathStep::Emit(event) => return Some(event),
                RhoPathStep::Node { cursor, field } => {
                    if field == 0 {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveNode));
                    }
                    let meta = cursor.program().fields.get(field);
                    if field == 0 && meta.is_none() {
                        return Some(RhoPathEvent::EnterNode {
                            cursor,
                            program: cursor.program(),
                        });
                    }
                    let Some(meta) = meta else { continue };
                    let value = cursor
                        .field(field)
                        .expect("generated cursor program and accessor must have equal lengths");
                    self.work.push(RhoPathStep::Node {
                        cursor,
                        field: field + 1,
                    });
                    self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveField));
                    self.work.push(RhoPathStep::Value(value));
                    self.work
                        .push(RhoPathStep::Emit(RhoPathEvent::EnterField(meta)));
                    if field == 0 {
                        return Some(RhoPathEvent::EnterNode {
                            cursor,
                            program: cursor.program(),
                        });
                    }
                }
                RhoPathStep::Value(value) => match value {
                    RhoMatchValue::Scalar(value) => return Some(RhoPathEvent::Scalar(value)),
                    RhoMatchValue::OptionalMessage(Some(cursor)) => {
                        self.work.push(RhoPathStep::Node { cursor, field: 0 });
                    }
                    RhoMatchValue::OptionalMessage(None) => return Some(RhoPathEvent::Absent),
                    RhoMatchValue::MessageSequence(cursor) => {
                        self.work
                            .push(RhoPathStep::Emit(RhoPathEvent::LeaveSequence));
                        self.work.push(RhoPathStep::Sequence { cursor, index: 0 });
                        return Some(RhoPathEvent::EnterSequence(cursor.len()));
                    }
                    RhoMatchValue::StringMessageMap(entries) => {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveMap));
                        self.work.push(RhoPathStep::Map {
                            entries: entries.iter(),
                        });
                        return Some(RhoPathEvent::EnterMap(entries.len()));
                    }
                    RhoMatchValue::Oneof(Some(cursor)) => {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveOneof));
                        self.work.push(RhoPathStep::Oneof(cursor));
                        return Some(RhoPathEvent::EnterOneof(cursor));
                    }
                    RhoMatchValue::Oneof(None) => return Some(RhoPathEvent::Absent),
                    RhoMatchValue::ParBag(bag) => return Some(RhoPathEvent::ParBag(bag)),
                    RhoMatchValue::PathMap(map) => return Some(RhoPathEvent::PathMap(map)),
                },
                RhoPathStep::Sequence { cursor, index } => {
                    let Some(child) = cursor.get(index) else {
                        continue;
                    };
                    self.work.push(RhoPathStep::Sequence {
                        cursor,
                        index: index + 1,
                    });
                    self.work.push(RhoPathStep::Node {
                        cursor: child,
                        field: 0,
                    });
                }
                RhoPathStep::Map { mut entries } => {
                    let Some((key, value)) = entries.next() else {
                        continue;
                    };
                    self.work.push(RhoPathStep::Map { entries });
                    self.work.push(RhoPathStep::Node {
                        cursor: ParCursor::Par(value),
                        field: 0,
                    });
                    return Some(RhoPathEvent::MapKey(key));
                }
                RhoPathStep::Oneof(cursor) => match cursor.payload {
                    RhoOneofPayload::Scalar(value) => return Some(RhoPathEvent::Scalar(value)),
                    RhoOneofPayload::Message(cursor) => {
                        self.work.push(RhoPathStep::Node { cursor, field: 0 });
                    }
                },
            }
        }
    }
}
