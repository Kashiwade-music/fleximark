//! Role: Convert native rendering values to checked external browser payloads.
use fleximark_model as model;
use fleximark_protocol as wire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionError(pub u64);
impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "integer {} exceeds the JSON transport range", self.0)
    }
}
impl std::error::Error for ConversionError {}

pub fn counter(value: u64) -> Result<wire::JsSafeU64, ConversionError> {
    wire::JsSafeU64::new(value).map_err(|_| ConversionError(value))
}

pub fn source_range(range: &model::SourceRange) -> Result<wire::SourceRange, ConversionError> {
    fn position(position: &model::SourcePosition) -> Result<wire::SourcePosition, ConversionError> {
        Ok(wire::SourcePosition {
            line: counter(position.line)?,
            character: counter(position.character)?,
            encoding: match position.encoding {
                model::PositionEncoding::Utf8 => wire::PositionEncoding::Utf8,
                model::PositionEncoding::Utf16 => wire::PositionEncoding::Utf16,
                model::PositionEncoding::Utf32 => wire::PositionEncoding::Utf32,
            },
        })
    }
    Ok(wire::SourceRange {
        byte_start: counter(range.byte_start)?,
        byte_end: counter(range.byte_end)?,
        start: position(&range.start)?,
        end: position(&range.end)?,
    })
}

pub fn frame(
    frame: &fleximark_application::RenderFrame,
) -> Result<wire::RenderFrame, ConversionError> {
    Ok(wire::RenderFrame {
        preview_session_id: wire::PreviewSessionId(frame.preview_session_id.0.clone()),
        document_version: counter(frame.document_version)?,
        render_revision: counter(frame.render_revision)?,
        renderer_fingerprint: frame.renderer_fingerprint.clone(),
        style: frame.style.as_ref().map(|style| wire::RenderStyle {
            css: style.css().to_owned(),
            fingerprint: style.fingerprint().to_owned(),
        }),
        assets: frame
            .assets
            .iter()
            .map(|asset| {
                Ok(wire::RenderAsset {
                    reference: asset.reference.clone(),
                    media_type: asset.media_type.clone(),
                    content_hash: asset.content_hash.clone(),
                    byte_length: counter(asset.byte_length)?,
                    data: asset.data.clone(),
                })
            })
            .collect::<Result<Vec<_>, ConversionError>>()?,
        blocks: frame
            .blocks
            .iter()
            .map(|block| wire::RenderBlock {
                id: wire::NodeId(block.id.0.clone()),
                html: block.html.clone(),
                node_ids: block
                    .node_ids
                    .iter()
                    .map(|id| wire::NodeId(id.0.clone()))
                    .collect(),
            })
            .collect(),
        navigation: frame
            .navigation
            .iter()
            .map(|entry| {
                Ok(wire::NavigationEntry {
                    node_id: wire::NodeId(entry.node_id.0.clone()),
                    source_range: source_range(&entry.source_range)?,
                    depth: entry.depth,
                })
            })
            .collect::<Result<Vec<_>, ConversionError>>()?,
        annotations: frame.annotations.clone(),
    })
}
#[cfg(test)]
mod tests {
    //! Role: Verify native values are rejected at the JSON boundary without panic.
    use super::*;

    #[test]
    fn counters_reject_values_above_the_external_integer_range() {
        let maximum = wire::MAX_SAFE_INTEGER as u64;
        assert_eq!(counter(maximum).unwrap().get(), maximum);
        assert_eq!(counter(maximum + 1), Err(ConversionError(maximum + 1)));
    }

    #[test]
    fn frames_and_source_positions_propagate_integer_overflow() {
        let position = model::SourcePosition {
            line: u64::MAX,
            character: 0,
            encoding: model::PositionEncoding::Utf16,
        };
        let range = model::SourceRange {
            byte_start: 0,
            byte_end: 1,
            start: position.clone(),
            end: position,
        };
        assert_eq!(source_range(&range), Err(ConversionError(u64::MAX)));
        let native = fleximark_application::RenderFrame {
            preview_session_id: fleximark_application::PreviewSessionId("preview".into()),
            document_version: 1,
            render_revision: u64::MAX,
            renderer_fingerprint: "test".into(),
            style: None,
            assets: Vec::new(),
            blocks: Vec::new(),
            navigation: Vec::new(),
            annotations: Default::default(),
        };
        assert_eq!(frame(&native), Err(ConversionError(u64::MAX)));
    }
}
