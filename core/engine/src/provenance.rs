//! Role: Remap plugin-preprocessed source provenance to original source ranges.
use fleximark_model::{
    AnchorAffinity, Block, Document, GeneratedAnchor, Inline, InlineKind, Node, SourceIndex,
    SourceProvenance, TransformId,
};
use fleximark_plugin_api::{EditOrigin, PreprocessedSource};

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
    let source_index = SourceIndex::new(original_source);
    let source_range =
        |byte_start: u64, byte_end: u64| -> Result<fleximark_model::SourceRange, EngineError> {
            let range = fleximark_model::SourceRange {
                byte_start,
                byte_end,
            };
            source_index.validate_range(&range).map_err(|error| {
                EngineError::Plugin(format!("invalid preprocess origin: {error}"))
            })?;
            Ok(range)
        };
    let range_start = range.byte_start;
    let range_end = range.byte_end;
    for segment in &preprocessed.segments {
        let empty = range.byte_start == range.byte_end;
        let contains_empty = segment.output_start <= range_start
            && (range_start < segment.output_end
                || (range_start == preprocessed.text.len() as u64
                    && segment.output_end == range_start));
        if (!empty && (segment.output_start >= range_end || segment.output_end <= range_start))
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
                let overlap_start = segment.output_start.max(range_start);
                let overlap_end = segment.output_end.min(range_end);
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
                        let origin_start = origin_range.byte_start;
                        let length = origin_range.byte_end - origin_start;
                        if empty
                            && (relative_start < origin_offset + length
                                || (relative_start == origin_length && index + 1 == origin.len()))
                        {
                            let offset = origin_start + relative_start - origin_offset;
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
                        let byte_start = origin_start + start - (origin_offset - length);
                        let byte_end = origin_start + end - (origin_offset - length);
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
