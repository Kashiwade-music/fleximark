//! Role: Convert native rendering values to checked external browser payloads.
use fleximark_model as model;
use fleximark_protocol as wire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    IntegerRange(u64),
    InvalidSourceRange,
}
impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IntegerRange(value) => {
                write!(f, "integer {value} exceeds the JSON transport range")
            }
            Self::InvalidSourceRange => {
                f.write_str("source range is invalid for the original document")
            }
        }
    }
}
impl std::error::Error for ConversionError {}

pub fn counter(value: u64) -> Result<wire::JsSafeU64, ConversionError> {
    wire::JsSafeU64::new(value).map_err(|_| ConversionError::IntegerRange(value))
}

pub fn source_range(
    range: &model::SourceRange,
    source: &model::SourceIndex,
    encoding: model::PositionEncoding,
) -> Result<wire::SourceRange, ConversionError> {
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
        start: position(
            &source
                .byte_to_position(
                    usize::try_from(range.byte_start)
                        .map_err(|_| ConversionError::InvalidSourceRange)?,
                    encoding,
                )
                .ok_or(ConversionError::InvalidSourceRange)?,
        )?,
        end: position(
            &source
                .byte_to_position(
                    usize::try_from(range.byte_end)
                        .map_err(|_| ConversionError::InvalidSourceRange)?,
                    encoding,
                )
                .ok_or(ConversionError::InvalidSourceRange)?,
        )?,
    })
}

pub fn frame(
    frame: &fleximark_application::RenderFrame,
) -> Result<wire::RenderFrame, ConversionError> {
    let source = model::SourceIndex::new(&frame.source);
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
                    source_range: source_range(
                        &entry.source_range,
                        &source,
                        frame.position_encoding,
                    )?,
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
        assert_eq!(
            counter(maximum + 1),
            Err(ConversionError::IntegerRange(maximum + 1))
        );
    }

    #[test]
    fn frames_and_source_positions_propagate_integer_overflow() {
        let range = model::SourceRange {
            byte_start: u64::MAX,
            byte_end: u64::MAX,
        };
        assert_eq!(
            source_range(
                &range,
                &model::SourceIndex::new("x"),
                model::PositionEncoding::Utf8
            ),
            Err(ConversionError::IntegerRange(u64::MAX))
        );
        let native = fleximark_application::RenderFrame::from_snapshot(
            fleximark_application::PreviewSessionId("preview".into()),
            u64::MAX,
            fleximark_engine::RenderSnapshot {
                document_version: 1,
                renderer_fingerprint: "test".into(),
                style: None,
                assets: Vec::new(),
                blocks: Vec::new(),
                navigation: Vec::new(),
                annotations: Default::default(),
            },
        );
        assert_eq!(frame(&native), Err(ConversionError::IntegerRange(u64::MAX)));
    }
}
