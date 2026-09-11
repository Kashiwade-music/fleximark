use fleximark_model::{PositionEncoding, SourcePosition, SourceProvenance, SourceRange};
use fleximark_plugin_sdk::{EditOrigin, PreprocessedSource};

pub(super) fn identity_edit_map(source: &str) -> Vec<fleximark_plugin_sdk::EditMapSegment> {
    if source.is_empty() {
        return Vec::new();
    }
    let (line, character) = source.rfind('\n').map_or((0, source.len()), |index| {
        (
            source[..=index]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            source.len() - index - 1,
        )
    });
    vec![fleximark_plugin_sdk::EditMapSegment {
        output_start: 0,
        output_end: source.len() as u64,
        origin: EditOrigin::Original {
            ranges: vec![SourceRange {
                byte_start: 0,
                byte_end: source.len() as u64,
                start: SourcePosition {
                    line: 0,
                    character: 0,
                    encoding: PositionEncoding::Utf8,
                },
                end: SourcePosition {
                    line: line as u64,
                    character: character as u64,
                    encoding: PositionEncoding::Utf8,
                },
            }],
            primary_range_index: 0,
        },
    }]
}

pub(super) fn validate_edit_map(
    candidate: &PreprocessedSource,
    source: &str,
) -> Result<(), String> {
    if candidate.text != source && candidate.segments.is_empty() {
        return Err("changed source requires a complete edit map".to_owned());
    }
    let mut expected_start = 0_u64;
    for segment in &candidate.segments {
        if segment.output_start != expected_start || segment.output_end <= segment.output_start {
            return Err("edit map segments must be contiguous and non-empty".to_owned());
        }
        let start = usize::try_from(segment.output_start)
            .map_err(|_| "edit map offset does not fit this platform".to_owned())?;
        let end = usize::try_from(segment.output_end)
            .map_err(|_| "edit map offset does not fit this platform".to_owned())?;
        if !candidate.text.is_char_boundary(start) || !candidate.text.is_char_boundary(end) {
            return Err("edit map offsets must be UTF-8 boundaries".to_owned());
        }
        validate_edit_origin(&segment.origin, source, &candidate.text[start..end])?;
        expected_start = segment.output_end;
    }
    if !candidate.segments.is_empty()
        && expected_start != u64::try_from(candidate.text.len()).unwrap_or(u64::MAX)
    {
        return Err("edit map must cover the complete output".to_owned());
    }
    Ok(())
}

pub(super) fn compose_edit_map(
    candidate: &PreprocessedSource,
    previous: &PreprocessedSource,
    original: &str,
) -> Result<PreprocessedSource, String> {
    if candidate.segments.is_empty() && candidate.text == previous.text {
        return Ok(previous.clone());
    }
    let mut segments = Vec::new();
    for segment in &candidate.segments {
        match &segment.origin {
            EditOrigin::Original { ranges, .. } => {
                let mut output = segment.output_start;
                for range in ranges {
                    for (length, origin) in map_input_range(range, previous, original)? {
                        let output_end = output + length;
                        segments.push(fleximark_plugin_sdk::EditMapSegment {
                            output_start: output,
                            output_end,
                            origin,
                        });
                        output = output_end;
                    }
                }
                if output != segment.output_end {
                    return Err("composed Original edit-map length changed".to_owned());
                }
            }
            EditOrigin::Derived {
                ranges,
                primary_range_index,
            } => {
                let mut mapped = Vec::new();
                let mut primary_start = None;
                for (index, range) in ranges.iter().enumerate() {
                    let before = mapped.len();
                    for (_, origin) in map_input_range(range, previous, original)? {
                        match origin {
                            EditOrigin::Original { ranges, .. }
                            | EditOrigin::Derived { ranges, .. } => mapped.extend(ranges),
                            EditOrigin::Generated {
                                anchor: Some(anchor),
                            } => mapped.push(anchor),
                            EditOrigin::Generated { anchor: None } => {}
                        }
                    }
                    if index == *primary_range_index as usize {
                        primary_start = (before < mapped.len()).then_some(mapped[before].clone());
                    }
                }
                mapped.sort_by_key(|range| (range.byte_start, range.byte_end));
                mapped.dedup_by_key(|range| (range.byte_start, range.byte_end));
                if mapped
                    .windows(2)
                    .any(|pair| pair[0].byte_end > pair[1].byte_start)
                {
                    return Err("composed Derived ranges overlap".to_owned());
                }
                let origin = if mapped.is_empty() {
                    EditOrigin::Generated { anchor: None }
                } else {
                    let primary = primary_start
                        .and_then(|primary| mapped.iter().position(|range| range == &primary))
                        .unwrap_or(0) as u32;
                    EditOrigin::Derived {
                        ranges: mapped,
                        primary_range_index: primary,
                    }
                };
                segments.push(fleximark_plugin_sdk::EditMapSegment {
                    output_start: segment.output_start,
                    output_end: segment.output_end,
                    origin,
                });
            }
            EditOrigin::Generated { anchor } => {
                let anchor = anchor
                    .as_ref()
                    .map(|range| map_input_range(range, previous, original))
                    .transpose()?
                    .and_then(|origins| {
                        origins.into_iter().find_map(|(_, origin)| match origin {
                            EditOrigin::Original { ranges, .. }
                            | EditOrigin::Derived { ranges, .. } => ranges.into_iter().next(),
                            EditOrigin::Generated { anchor } => anchor,
                        })
                    });
                segments.push(fleximark_plugin_sdk::EditMapSegment {
                    output_start: segment.output_start,
                    output_end: segment.output_end,
                    origin: EditOrigin::Generated { anchor },
                });
            }
        }
    }
    let composed = PreprocessedSource {
        text: candidate.text.clone(),
        segments,
    };
    validate_edit_map(&composed, original)?;
    Ok(composed)
}

fn map_input_range(
    range: &SourceRange,
    previous: &PreprocessedSource,
    original: &str,
) -> Result<Vec<(u64, EditOrigin)>, String> {
    let mut mapped = Vec::new();
    for segment in &previous.segments {
        let start = range.byte_start.max(segment.output_start);
        let end = range.byte_end.min(segment.output_end);
        if start >= end {
            continue;
        }
        let origin = match &segment.origin {
            EditOrigin::Original {
                ranges,
                primary_range_index: _,
            } => {
                let mut skip = start - segment.output_start;
                let mut take = end - start;
                let mut clipped = Vec::new();
                for source_range in ranges {
                    let length = source_range.byte_end - source_range.byte_start;
                    if skip >= length {
                        skip -= length;
                        continue;
                    }
                    let clipped_start = source_range.byte_start + skip;
                    let clipped_end = clipped_start + take.min(length - skip);
                    clipped.push(utf8_source_range(original, clipped_start, clipped_end)?);
                    take -= clipped_end - clipped_start;
                    skip = 0;
                    if take == 0 {
                        break;
                    }
                }
                if take != 0 {
                    return Err("Original edit-map range could not be clipped".to_owned());
                }
                EditOrigin::Original {
                    ranges: clipped,
                    primary_range_index: 0,
                }
            }
            EditOrigin::Derived {
                ranges,
                primary_range_index,
            } => EditOrigin::Derived {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
            },
            EditOrigin::Generated { anchor } => EditOrigin::Generated {
                anchor: anchor.clone(),
            },
        };
        mapped.push((end - start, origin));
    }
    if mapped.iter().map(|(length, _)| *length).sum::<u64>() != range.byte_end - range.byte_start {
        return Err("edit-map range is not covered by the previous output".to_owned());
    }
    Ok(mapped)
}

fn utf8_source_range(source: &str, start: u64, end: u64) -> Result<SourceRange, String> {
    let position = |offset: u64| -> Result<SourcePosition, String> {
        let offset = usize::try_from(offset).map_err(|_| "source offset is too large")?;
        if !source.is_char_boundary(offset) {
            return Err("source offset is not a UTF-8 boundary".to_owned());
        }
        let before = &source[..offset];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        Ok(SourcePosition {
            line: before.bytes().filter(|byte| *byte == b'\n').count() as u64,
            character: (offset - line_start) as u64,
            encoding: PositionEncoding::Utf8,
        })
    };
    Ok(SourceRange {
        byte_start: start,
        byte_end: end,
        start: position(start)?,
        end: position(end)?,
    })
}

#[cfg(test)]
pub(super) fn test_utf8_source_range(
    source: &str,
    start: u64,
    end: u64,
) -> Result<SourceRange, String> {
    utf8_source_range(source, start, end)
}

fn validate_edit_origin(origin: &EditOrigin, source: &str, output: &str) -> Result<(), String> {
    match origin {
        EditOrigin::Original {
            ranges,
            primary_range_index,
        } => {
            SourceProvenance::Original {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
            }
            .validate(source)
            .map_err(|error| error.to_string())?;
            let mut original = Vec::new();
            for range in ranges {
                let start = usize::try_from(range.byte_start)
                    .map_err(|_| "origin offset does not fit this platform".to_owned())?;
                let end = usize::try_from(range.byte_end)
                    .map_err(|_| "origin offset does not fit this platform".to_owned())?;
                original.extend_from_slice(&source.as_bytes()[start..end]);
            }
            if original != output.as_bytes() {
                return Err(
                    "an Original edit-map segment must exactly reproduce its source ranges"
                        .to_owned(),
                );
            }
        }
        EditOrigin::Derived {
            ranges,
            primary_range_index,
        } => {
            SourceProvenance::Derived {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
                transform: fleximark_model::TransformId("plugin-preprocess-v1".to_owned()),
            }
            .validate(source)
            .map_err(|error| error.to_string())?;
        }
        EditOrigin::Generated { anchor } => {
            if let Some(range) = anchor {
                SourceProvenance::original(range.clone())
                    .validate(source)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}
