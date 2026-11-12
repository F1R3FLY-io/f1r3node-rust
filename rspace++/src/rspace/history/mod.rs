pub mod history;
pub mod history_reader;
pub mod history_repository;
pub mod history_action;
pub mod instances;
pub mod radix_tree;
pub(crate) mod roots_store;
pub use roots_store::CurrentRoot;
pub mod cold_store;
pub mod history_repository_impl;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only unit tests read a durable history repository's view through its reader \
                  accessor; the repository holds the reader for its generation lease"
    )
)]
pub(crate) mod durable_history_repository;

// PartialEq needed here for testing purposes
#[derive(Debug, PartialEq, Clone)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    pub fn map<F, T>(self, f: F) -> Either<L, T>
    where F: FnOnce(R) -> T {
        match self {
            Either::Left(l) => Either::Left(l),
            Either::Right(r) => Either::Right(f(r)),
        }
    }

    pub fn and_then<F, T>(self, f: F) -> Either<L, T>
    where F: FnOnce(R) -> Either<L, T> {
        match self {
            Either::Left(l) => Either::Left(l),
            Either::Right(r) => f(r),
        }
    }

    pub fn left_then<F>(self, f: F) -> Either<L, R>
    where F: FnOnce(L) -> Either<L, R> {
        match self {
            Either::Left(l) => f(l),
            Either::Right(r) => Either::Right(r),
        }
    }
}
