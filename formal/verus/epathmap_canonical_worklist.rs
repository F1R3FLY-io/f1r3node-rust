use vstd::prelude::*;

verus! {

#[derive(Copy, Clone)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

pub fn relative_region(outer: ByteRange, inner: ByteRange) -> (result: ByteRange)
    requires
        outer.start <= outer.end,
        inner.start <= inner.end,
        inner.end <= outer.end - outer.start,
    ensures
        result.start == outer.start + inner.start,
        result.end == outer.start + inner.end,
        outer.start <= result.start,
        result.start <= result.end,
        result.end <= outer.end,
{
    ByteRange {
        start: outer.start + inner.start,
        end: outer.start + inner.end,
    }
}

pub open spec fn recursive_validation(results: Seq<bool>) -> bool
    decreases results.len(),
{
    if results.len() == 0 {
        true
    } else {
        results[0] && recursive_validation(results.drop_first())
    }
}

pub open spec fn worklist_validation(results: Seq<bool>, cursor: int) -> bool
    recommends 0 <= cursor <= results.len(),
    decreases results.len() - cursor,
{
    if cursor >= results.len() {
        true
    } else {
        results[cursor] && worklist_validation(results, cursor + 1)
    }
}

proof fn validation_suffix_equivalent(results: Seq<bool>, cursor: int)
    requires 0 <= cursor <= results.len(),
    ensures
        worklist_validation(results, cursor)
            == recursive_validation(results.subrange(cursor, results.len() as int)),
    decreases results.len() - cursor,
{
    if cursor < results.len() {
        validation_suffix_equivalent(results, cursor + 1);
        assert(results.subrange(cursor, results.len() as int).drop_first()
            == results.subrange(cursor + 1, results.len() as int));
    }
}

proof fn canonical_validation_worklist_equivalent(results: Seq<bool>)
    ensures worklist_validation(results, 0) == recursive_validation(results),
{
    validation_suffix_equivalent(results, 0);
    assert(results.subrange(0, results.len() as int) == results);
}

}

fn main() {}
