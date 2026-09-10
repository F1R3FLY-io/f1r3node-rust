#![allow(dead_code)]

use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieIndexedTopologyCursor as Cursor, CanonicalCompactTrieLineSpan as Span,
    CanonicalCompactTriePreparedLineSpan as Prepared, IndexedByteSource,
};

fn released_offer_allows_fresh_cursor_access<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    if let Some(span) = cursor.line_span() {
        let width = span.source_range().len();
        let _ = span.commit(width);
    }
    cursor.rewind();
    let _ = cursor.next_event();
}

fn released_preparation_allows_fresh_cursor_access<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    if let Some(span) = cursor.line_span() {
        if let Some(prepared) = span.prepare(1) {
            let _ = prepared.source();
            let _ = prepared.source_range();
            let _ = prepared.direction();
            let _ = prepared.expanded_events();
            let _ = prepared.last_source_offset();
            let _ = prepared.commit();
        }
    }
    cursor.rewind();
    if let Some(span) = cursor.line_span() {
        drop(span.prepare(1));
    }
    let _ = cursor.next_event();
}

#[cfg(live_rewind)]
fn live_rewind<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let span = cursor.line_span().unwrap();
    cursor.rewind();
    let _ = span.commit(1);
}

#[cfg(live_next_event)]
fn live_next_event<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let span = cursor.line_span().unwrap();
    let _ = cursor.next_event();
    let _ = span.commit(1);
}

#[cfg(live_replace)]
fn live_replace<S: IndexedByteSource>(cursor: &mut Cursor<S>, replacement: Cursor<S>) {
    let span = cursor.line_span().unwrap();
    *cursor = replacement;
    let _ = span.commit(1);
}

#[cfg(double_commit)]
fn double_commit<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let span = cursor.line_span().unwrap();
    let _ = span.commit(1);
    let _ = span.commit(1);
}

#[cfg(clone_offer)]
fn clone_offer<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let span = cursor.line_span().unwrap();
    let _ = span.clone();
}

#[cfg(copy_offer)]
fn copy_offer<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    fn requires_copy<T: Copy>(_: T) {}
    requires_copy(cursor.line_span().unwrap());
}

#[cfg(escape_cursor)]
fn escape_cursor<S: IndexedByteSource + 'static>(source: S) -> Span<'static, S> {
    let mut cursor = Cursor::from_source(source).unwrap();
    cursor.line_span().unwrap()
}

#[cfg(substitute_cursor)]
fn substitute_cursor<'a, S: IndexedByteSource>(mut span: Span<'a, S>, other: &'a mut Cursor<S>) {
    span.cursor = other;
    let _ = span.commit(1);
}

#[cfg(prepared_live_rewind)]
fn prepared_live_rewind<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let prepared = cursor.line_span().unwrap().prepare(1).unwrap();
    cursor.rewind();
    let _ = prepared.commit();
}

#[cfg(prepared_live_next_event)]
fn prepared_live_next_event<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let prepared = cursor.line_span().unwrap().prepare(1).unwrap();
    let _ = cursor.next_event();
    let _ = prepared.commit();
}

#[cfg(prepared_live_replace)]
fn prepared_live_replace<S: IndexedByteSource>(cursor: &mut Cursor<S>, replacement: Cursor<S>) {
    let prepared = cursor.line_span().unwrap().prepare(1).unwrap();
    *cursor = replacement;
    let _ = prepared.commit();
}

#[cfg(prepared_double_commit)]
fn prepared_double_commit<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let prepared = cursor.line_span().unwrap().prepare(1).unwrap();
    let _ = prepared.commit();
    let _ = prepared.commit();
}

#[cfg(prepared_clone_offer)]
fn prepared_clone_offer<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    let prepared = cursor.line_span().unwrap().prepare(1).unwrap();
    let _ = prepared.clone();
}

#[cfg(prepared_copy_offer)]
fn prepared_copy_offer<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    fn requires_copy<T: Copy>(_: T) {}
    requires_copy(cursor.line_span().unwrap().prepare(1).unwrap());
}

#[cfg(prepared_escape_cursor)]
fn prepared_escape_cursor<S: IndexedByteSource + 'static>(source: S) -> Prepared<'static, S> {
    let mut cursor = Cursor::from_source(source).unwrap();
    cursor.line_span().unwrap().prepare(1).unwrap()
}

#[cfg(prepared_substitute_source)]
fn prepared_substitute_source<'a, S: IndexedByteSource>(mut prepared: Prepared<'a, S>, source: &'a S) {
    prepared.source = source;
    let _ = prepared.commit();
}
