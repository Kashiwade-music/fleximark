//! Role: Read canonical product and protocol versions for runtime consumers.
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseMetadata {
    pub version: String,
    pub protocol_version: u32,
}

impl ReleaseMetadata {
    pub fn parse(source: &str) -> io::Result<Self> {
        let mut version = None;
        let mut protocol_version = None;
        for line in source.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| io::Error::other("invalid release metadata"))?;
            match key.trim() {
                "version" if version.is_none() => {
                    let value = value.trim();
                    let value = value
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                        .filter(|value| valid_version(value))
                        .ok_or_else(|| io::Error::other("invalid release version"))?;
                    version = Some(value.to_owned());
                }
                "protocol_version" if protocol_version.is_none() => {
                    let value = value
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or_else(|| io::Error::other("invalid protocol version"))?;
                    protocol_version = Some(value);
                }
                _ => {
                    return Err(io::Error::other(
                        "unknown or repeated release metadata field",
                    ));
                }
            }
        }
        Ok(Self {
            version: version.ok_or_else(|| io::Error::other("missing release version"))?,
            protocol_version: protocol_version
                .ok_or_else(|| io::Error::other("missing protocol version"))?,
        })
    }
}

pub fn release_metadata() -> io::Result<ReleaseMetadata> {
    ReleaseMetadata::parse(include_str!("../../../release.toml"))
}

pub fn valid_version(version: &str) -> bool {
    let (version, build) = version
        .split_once('+')
        .map_or((version, None), |(version, build)| (version, Some(build)));
    let (core, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(core, pre)| (core, Some(pre)));
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
        && [prerelease, build].into_iter().all(|value| {
            value.is_none_or(|value| {
                value.split('.').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            })
        })
}
