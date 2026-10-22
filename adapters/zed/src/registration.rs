//! Role: Register the extension with official Zed WASI ABI glue.

use crate::adapter::Fleximark;
use zed_extension_api as zed;

zed::register_extension!(Fleximark);
