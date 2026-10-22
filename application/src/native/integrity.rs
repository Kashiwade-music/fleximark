//! Role: Define filesystem object identities and content digests.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectIdentity {
    pub(crate) platform_id: String,
    pub(crate) kind: String,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
