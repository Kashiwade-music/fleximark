use std::collections::BTreeMap;
use std::sync::Arc;

use ed25519_dalek::{Signature, VerifyingKey};
use fleximark_plugin_sdk::{PluginCapabilities, PluginManifest};
use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::runtime::PluginRuntime;

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn verify_signature(
    public_key: &str,
    signature: &[u8],
    manifest: &[u8],
) -> Result<(), HostError> {
    let key_bytes = decode_hex::<32>(public_key)
        .ok_or_else(|| HostError::Integrity("invalid Ed25519 public key".to_owned()))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| HostError::Integrity("invalid Ed25519 public key".to_owned()))?;
    let signature = Signature::from_slice(signature)
        .map_err(|_| HostError::Integrity("invalid Ed25519 signature encoding".to_owned()))?;
    key.verify_strict(manifest, &signature)
        .map_err(|_| HostError::Integrity("plugin signature verification failed".to_owned()))
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut output = [0; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        output[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(output)
}

pub struct VerifiedPluginPackage<'a> {
    pub configured_id: &'a str,
    pub config_order: usize,
    pub manifest_bytes: &'a [u8],
    pub expected_manifest_sha256: &'a str,
    pub wasm_bytes: &'a [u8],
    pub signature_bytes: &'a [u8],
    pub signer_public_key: &'a str,
    pub grants: PluginCapabilities,
    pub environment: BTreeMap<String, String>,
}

pub(super) struct RegisteredPlugin {
    pub(super) manifest: PluginManifest,
    pub(super) grants: PluginCapabilities,
    pub(super) manifest_hash: String,
    pub(super) wasm_hash: String,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) runtime: Arc<dyn PluginRuntime>,
}
