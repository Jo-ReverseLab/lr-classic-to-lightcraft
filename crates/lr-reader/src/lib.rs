//! Reads Lightroom Classic catalogs into the neutral model.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

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
pub use originals::{
    CloudOnly, check_originals, check_originals_with, drop_missing_originals, flags_dataless,
    is_dataless,
};
pub use photos::{PhotoData, VIDEO_SKIP_REASON};
pub use smart::{SmartParseError, parse_smart_rules};
pub use stacks::StackData;
