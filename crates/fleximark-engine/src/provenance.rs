use fleximark_model::{
    AnchorAffinity, Block, Document, GeneratedAnchor, Inline, InlineKind, Node, PositionEncoding,
    SourcePosition, SourceProvenance, TransformId,
};
use fleximark_plugin_sdk::{EditOrigin, PreprocessedSource};

use crate::error::EngineError;

pub(super) fn remap_document_provenance(
    document: &mut Document,
    preprocessed: &PreprocessedSource,
    original_source: &str,
) -> Result<(), EngineError> {
    fn remap_inline(
        inline: &mut Inline,
        preprocessed: &PreprocessedSource,
        original_source: &str,
    ) -> Result<(), EngineError> {
        inline.provenance = remap_provenance(&inline.provenance, preprocessed, original_source)?;
        let children = match &mut inline.kind {
            InlineKind::Emphasis { children }
            | InlineKind::Strong { children }
            | InlineKind::Strikethrough { children }
            | InlineKind::Link { children, .. }
            | InlineKind::Image { children, .. } => children,
            _ => return Ok(()),
        };
        for child in children {
            remap_inline(child, preprocessed, original_source)?;
        }
        Ok(())
    }

    fn remap_blocks(
        blocks: &mut [Block],
        preprocessed: &PreprocessedSource,
        original_source: &str,
    ) -> Result<(), EngineError> {
        for block in blocks {
            block.provenance = remap_provenance(&block.provenance, preprocessed, original_source)?;
            for child in &mut block.children {
                match child {
                    Node::Block(block) => {
                        remap_blocks(std::slice::from_mut(block), preprocessed, original_source)?
                    }
                    Node::Inline(inline) => remap_inline(inline, preprocessed, original_source)?,
                }
            }
        }
        Ok(())
    }

    remap_blocks(&mut document.blocks, preprocessed, original_source)
}

fn remap_provenance(
    provenance: &SourceProvenance,
    preprocessed: &PreprocessedSource,
    original_source: &str,
) -> Result<SourceProvenance, EngineError> {
    let range = provenance
        .primary_range()
        .ok_or_else(|| EngineError::Plugin("parser provenance has no range to map".to_owned()))?;
    let mut ranges = Vec::new();
    let mut primary_range = None;
    let mut generated_anchor = None;
    let source_range = |byte_start: u64, byte_end: u64| {
        let start = usize::try_from(byte_start)
            .map_err(|_| EngineError::Plugin("preprocess origin offset overflowed".to_owned()))?;
        let end = usize::try_from(byte_end)
            .map_err(|_| EngineError::Plugin("preprocess origin offset overflowed".to_owned()))?;
        if !original_source.is_char_boundary(start) || !original_source.is_char_boundary(end) {
            return Err(EngineError::Plugin(
                "preprocess edit map cannot be clipped at UTF-8 boundaries".to_owned(),
            ));
        }
        let position = |offset: usize| {
            let before = &original_source[..offset];
            let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
            SourcePosition {
                line: before.bytes().filter(|byte| *byte == b'\n').count() as u64,
                character: (offset - line_start) as u64,
                encoding: PositionEncoding::Utf8,
            }
        };
        Ok(fleximark_model::SourceRange {
            byte_start,
            byte_end,
            start: position(start),
            end: position(end),
        })
    };
    for segment in &preprocessed.segments {
        let empty = range.byte_start == range.byte_end;
        let contains_empty = segment.output_start <= range.byte_start
            && (range.byte_start < segment.output_end
                || (range.byte_start == preprocessed.text.len() as u64
                    && segment.output_end == range.byte_start));
        if (!empty
            && (segment.output_start >= range.byte_end || segment.output_end <= range.byte_start))
            || (empty && !contains_empty)
        {
            continue;
        }
        match &segment.origin {
            EditOrigin::Original {
                ranges: origin,
                primary_range_index,
            }
            | EditOrigin::Derived {
                ranges: origin,
                primary_range_index,
            } => {
                let overlap_start = segment.output_start.max(range.byte_start);
                let overlap_end = segment.output_end.min(range.byte_end);
                let relative_start = overlap_start - segment.output_start;
                let relative_end = overlap_end - segment.output_start;
                let origin_length = origin
                    .iter()
                    .try_fold(0_u64, |length, range| {
                        length.checked_add(range.byte_end - range.byte_start)
                    })
                    .ok_or_else(|| {
                        EngineError::Plugin("preprocess origin length overflowed".to_owned())
                    })?;
                let exact = origin_length == segment.output_end - segment.output_start;
                if exact {
                    let mut origin_offset = 0_u64;
                    for (index, origin_range) in origin.iter().enumerate() {
                        let length = origin_range.byte_end - origin_range.byte_start;
                        if empty
                            && (relative_start < origin_offset + length
                                || (relative_start == origin_length && index + 1 == origin.len()))
                        {
                            let offset = origin_range.byte_start + relative_start - origin_offset;
                            let mapped = source_range(offset, offset)?;
                            if index == *primary_range_index as usize && primary_range.is_none() {
                                primary_range = Some(mapped.clone());
                            }
                            ranges.push(mapped);
                            break;
                        }
                        let start = relative_start.max(origin_offset);
                        let end = relative_end.min(origin_offset + length);
                        origin_offset += length;
                        if start >= end {
                            continue;
                        }
                        let byte_start = origin_range.byte_start + start - (origin_offset - length);
                        let byte_end = origin_range.byte_start + end - (origin_offset - length);
                        let mapped = source_range(byte_start, byte_end)?;
                        if index == *primary_range_index as usize && primary_range.is_none() {
                            primary_range = Some(mapped.clone());
                        }
                        ranges.push(mapped);
                    }
                } else {
                    for (index, origin_range) in origin.iter().enumerate() {
                        if index == *primary_range_index as usize && primary_range.is_none() {
                            primary_range = Some(origin_range.clone());
                        }
                        ranges.push(origin_range.clone());
                    }
                }
            }
            EditOrigin::Generated { anchor } => {
                generated_anchor = generated_anchor.or_else(|| anchor.clone());
            }
        }
    }
    if !ranges.is_empty() {
        ranges.sort_by_key(|range| (range.byte_start, range.byte_end));
        ranges.dedup();
        if ranges
            .windows(2)
            .any(|pair| pair[1].byte_start < pair[0].byte_end)
        {
            return Err(EngineError::Plugin(
                "mapped preprocess provenance overlaps in the original snapshot".to_owned(),
            ));
        }
        let primary_range_index = primary_range
            .and_then(|primary| ranges.iter().position(|range| range == &primary))
            .unwrap_or(0) as u32;
        return Ok(SourceProvenance::Derived {
            ranges,
            primary_range_index,
            transform: TransformId("plugin-preprocess-v1".to_owned()),
        });
    }
    if generated_anchor.is_some() {
        return Ok(SourceProvenance::Generated {
            anchor: generated_anchor.map(|range| GeneratedAnchor {
                range,
                affinity: AnchorAffinity::After,
            }),
            transform: TransformId("plugin-preprocess-v1".to_owned()),
        });
    }
    Err(EngineError::Plugin(
        "preprocess edit map does not cover parser provenance".to_owned(),
    ))
}

#[cfg(test)]
pub(super) fn remap_provenance_for_test(
    provenance: &SourceProvenance,
    preprocessed: &PreprocessedSource,
    original_source: &str,
) -> Result<SourceProvenance, EngineError> {
    remap_provenance(provenance, preprocessed, original_source)
}
