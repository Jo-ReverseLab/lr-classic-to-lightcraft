//! Reads Lightroom Classic catalogs into the neutral model.

mod catalog;
mod collections;
mod develop;
mod folders;
mod keywords;
mod originals;
mod photos;
mod smart;
mod stacks;
mod xmp;

pub use catalog::{Catalog, ReadError, SUPPORTED_VERSION, checksum};
pub use collections::CollectionData;
pub use develop::{DEVELOP_MAX_DEPTH, DEVELOP_MAX_TEXT_BYTES, DevelopError, parse_develop};
pub use keywords::KeywordData;
pub use originals::drop_missing_originals;
pub use photos::{PhotoData, VIDEO_SKIP_REASON};
pub use smart::{SmartParseError, parse_smart_rules};
pub use stacks::StackData;
