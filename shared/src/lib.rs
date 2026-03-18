// Upstream trait signatures use &Vec<> in public APIs; changing to &[_] would be a breaking change.
#![allow(clippy::ptr_arg)]
// Inherent `default()` methods used in upstream code.
#![allow(clippy::should_implement_trait)]
// Pattern matching style in upstream tests.
#![allow(clippy::redundant_pattern_matching)]

pub mod rust;
