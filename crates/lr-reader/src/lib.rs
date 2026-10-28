//! Reads Lightroom Classic catalogs into the neutral model.

mod catalog;
mod collections;
mod folders;
mod keywords;
mod originals;
mod photos;
mod stacks;
mod xmp;

pub use catalog::{Catalog, ReadError, SUPPORTED_VERSION, checksum};
pub use collections::CollectionData;
pub use keywords::KeywordData;
pub use originals::drop_missing_originals;
pub use photos::{PhotoData, VIDEO_SKIP_REASON};
pub use stacks::StackData;
