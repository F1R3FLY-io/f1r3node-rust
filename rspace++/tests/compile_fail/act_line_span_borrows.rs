#![allow(dead_code)]

use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieIndexedTopologyCursor as Cursor, CanonicalCompactTrieLineSpan as Span,
    IndexedByteSource,
};

fn released_offer_allows_fresh_cursor_access<S: IndexedByteSource>(cursor: &mut Cursor<S>) {
    if let Some(span) = cursor.line_span() {
        let width = span.source_range().len();
        let _ = span.commit(width);
    }
    cursor.rewind();
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
