//! Compatibility imports for feature-test modules. The implementation is
//! compiled once at the integration crate root instead of once per test file.
#![allow(unused_imports)]
pub use crate::test_support::*;
