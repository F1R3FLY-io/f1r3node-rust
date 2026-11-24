use std::collections::HashMap;
use std::sync::Arc;

use super::bound_context::BoundContext;
use super::id_context::{IdContextPos, IdContextSpan};

/// The bindings are shared between copies and copied on the first write, so
/// a scope's map can be handed to each of many sub-processes for the cost of
/// a reference count.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundMap<T> {
    next_index: usize,
    index_bindings: Arc<HashMap<String, BoundContext<T>>>,
}

impl<T: Clone> BoundMap<T> {
    pub fn new() -> Self {
        BoundMap {
            next_index: 0,
            index_bindings: Arc::new(HashMap::new()),
        }
    }

    pub fn get(&self, name: &str) -> Option<BoundContext<T>>
    where T: Clone {
        self.index_bindings.get(name).map(|context| BoundContext {
            index: self.next_index - context.index - 1,
            typ: context.typ.clone(),
            source_span: context.source_span,
        })
    }

    fn insert_span(&mut self, (name, typ, source_span): IdContextSpan<T>) {
        Arc::make_mut(&mut self.index_bindings).insert(name, BoundContext {
            index: self.next_index,
            typ,
            source_span,
        });
        self.next_index += 1;
    }

    fn span_of((name, typ, source_pos): IdContextPos<T>) -> IdContextSpan<T> {
        let source_span = rholang_parser::SourceSpan {
            start: source_pos,
            end: source_pos,
        };
        (name, typ, source_span)
    }

    /// Put binding with SourceSpan (for AnnProc, AnnName, etc.)
    pub fn put_span(&self, binding: IdContextSpan<T>) -> BoundMap<T> {
        self.put_all_span(vec![binding])
    }

    /// Put binding with SourcePos (for Id types) - converts to SourceSpan
    pub fn put_pos(&self, binding: IdContextPos<T>) -> BoundMap<T> {
        self.put_all_pos(vec![binding])
    }

    pub fn put_all_span(&self, bindings: Vec<IdContextSpan<T>>) -> BoundMap<T> {
        let mut new_map = self.clone();
        for binding in bindings {
            new_map.insert_span(binding);
        }
        new_map
    }

    pub fn put_all_pos(&self, bindings: Vec<IdContextPos<T>>) -> BoundMap<T> {
        self.put_all_span(bindings.into_iter().map(Self::span_of).collect())
    }

    pub fn absorb_free_span(&self, free_map: &FreeMap<T>) -> BoundMap<T> {
        let mut new_bindings = (*self.index_bindings).clone();
        for (name, context) in free_map.level_bindings.iter() {
            new_bindings.insert(name.clone(), BoundContext {
                index: context.level + self.next_index,
                typ: context.typ.clone(),
                source_span: context.source_span,
            });
        }
        BoundMap {
            next_index: self.next_index + free_map.next_level,
            index_bindings: Arc::new(new_bindings),
        }
    }

    pub fn get_count(&self) -> usize { self.next_index }
}

impl<T: Clone> Default for BoundMap<T> {
    fn default() -> Self { Self::new() }
}

// Forward declaration for FreeMapSpan
use super::free_map::FreeMap;
