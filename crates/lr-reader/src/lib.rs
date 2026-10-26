//! Reads Lightroom Classic catalogs into the neutral model.

mod catalog;
mod folders;

pub use catalog::{Catalog, ReadError, SUPPORTED_VERSION, checksum};
