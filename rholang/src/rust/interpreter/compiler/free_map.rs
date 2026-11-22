use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::connective::ConnectiveInstance;

use super::free_context::FreeContext;
use super::id_context::{IdContextPos, IdContextSpan};

/// The contents are shared between copies and copied on the first write, so
/// handing the map to each of many sub-processes costs a reference count.
#[derive(Debug, Clone, PartialEq)]
pub struct FreeMap<T: Clone> {
    pub next_level: usize,
    pub level_bindings: Arc<HashMap<String, FreeContext<T>>>,
    pub wildcards: Arc<Vec<rholang_parser::SourceSpan>>,
    pub connectives: Arc<Vec<(ConnectiveInstance, rholang_parser::SourceSpan)>>,
}

impl<T: Clone> FreeMap<T> {
    pub fn new() -> Self {
        FreeMap {
            next_level: 0,
            level_bindings: Arc::new(HashMap::new()),
            wildcards: Arc::new(Vec::new()),
            connectives: Arc::new(Vec::new()),
        }
    }

    pub fn get(&self, name: &str) -> Option<FreeContext<T>>
    where T: Clone {
        self.level_bindings.get(name).cloned()
    }

    /// Put binding with SourceSpan (for AnnProc, AnnName, etc.)
    pub fn put_span(mut self, (name, typ, source_span): IdContextSpan<T>) -> Self {
        Arc::make_mut(&mut self.level_bindings).insert(name, FreeContext {
            level: self.next_level,
            typ,
            source_span,
        });
        self.next_level += 1;
        self
    }

    /// Put binding with SourcePos (for Id types) - converts to SourceSpan
    pub fn put_pos(self, (name, typ, source_pos): IdContextPos<T>) -> Self {
        let source_span = rholang_parser::SourceSpan {
            start: source_pos,
            end: source_pos,
        };
        self.put_span((name, typ, source_span))
    }

    pub fn put_all_span(self, bindings: Vec<IdContextSpan<T>>) -> Self {
        bindings.into_iter().fold(self, Self::put_span)
    }

    pub fn put_all_pos(self, bindings: Vec<IdContextPos<T>>) -> Self {
        bindings.into_iter().fold(self, Self::put_pos)
    }

    /// Returns the new map, and the shadowed variables with the span each had
    /// before and the span that shadows it
    #[allow(clippy::type_complexity)]
    pub fn merge(
        mut self,
        free_map: FreeMap<T>,
    ) -> (
        FreeMap<T>,
        Vec<(
            String,
            rholang_parser::SourceSpan,
            rholang_parser::SourceSpan,
        )>,
    ) {
        let mut shadowed = Vec::new();
        let level_bindings = Arc::make_mut(&mut self.level_bindings);
        for (name, free_context) in Arc::unwrap_or_clone(free_map.level_bindings) {
            let source_span = free_context.source_span;
            let previous = level_bindings.insert(name.clone(), FreeContext {
                level: free_context.level + self.next_level,
                typ: free_context.typ,
                source_span,
            });
            if let Some(previous) = previous {
                shadowed.push((name, previous.source_span, source_span));
            }
        }
        shadowed.reverse();

        self.next_level += free_map.next_level;
        Arc::make_mut(&mut self.wildcards).extend(free_map.wildcards.iter().copied());
        Arc::make_mut(&mut self.connectives).extend(free_map.connectives.iter().cloned());
        (self, shadowed)
    }

    pub fn add_wildcard(mut self, source_span: rholang_parser::SourceSpan) -> Self {
        Arc::make_mut(&mut self.wildcards).push(source_span);
        self
    }

    pub fn add_connective(
        mut self,
        connective: ConnectiveInstance,
        source_span: rholang_parser::SourceSpan,
    ) -> Self {
        Arc::make_mut(&mut self.connectives).push((connective, source_span));
        self
    }

    pub fn count(&self) -> usize { self.next_level + self.wildcards.len() + self.connectives.len() }

    pub fn count_no_wildcards(&self) -> usize { self.next_level }
}

impl<T: Clone> Default for FreeMap<T> {
    fn default() -> Self { Self::new() }
}
