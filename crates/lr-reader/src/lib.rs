//! Reads Lightroom Classic catalogs into the neutral model.

mod catalog;

pub use catalog::{Catalog, ReadError, SUPPORTED_VERSION, checksum};
