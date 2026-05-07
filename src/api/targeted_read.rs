use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::api::schema::{PaneReadRegion, PaneResolvedRegion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetedReadRegion {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

pub(crate) fn crop_text_region(
    text: &str,
    requested: &PaneReadRegion,
    surface_width: Option<usize>,
    surface_height: Option<usize>,
) -> Result<(String, TargetedReadRegion), String> {
    let lines: Vec<&str> = text.lines().collect();
    let total_width = surface_width.unwrap_or_else(|| {
        lines
            .iter()
            .map(|line| UnicodeWidthStr::width(*line))
            .max()
            .unwrap_or(0)
    });
    let total_height = surface_height.unwrap_or(lines.len());

    let (left, right) = resolve_axis(
        total_width,
        requested.left,
        requested.right,
        requested.width,
        "x-axis",
        "left",
        "right",
        "width",
    )?;
    let (top, bottom) = resolve_axis(
        total_height,
        requested.top,
        requested.bottom,
        requested.height,
        "y-axis",
        "top",
        "bottom",
        "height",
    )?;

    let cropped = (top..bottom)
        .map(|row| crop_line_columns(lines.get(row).copied().unwrap_or(""), left, right))
        .collect::<Vec<_>>()
        .join("\n");

    Ok((
        cropped,
        TargetedReadRegion {
            left,
            top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        },
    ))
}

pub(crate) fn resolved_region_for_response(region: &TargetedReadRegion) -> PaneResolvedRegion {
    PaneResolvedRegion {
        left: region.left as u32,
        top: region.top as u32,
        width: region.width as u32,
        height: region.height as u32,
    }
}

fn resolve_axis(
    total: usize,
    start_inset: Option<u32>,
    end_inset: Option<u32>,
    size: Option<u32>,
    axis_name: &str,
    start_name: &str,
    end_name: &str,
    size_name: &str,
) -> Result<(usize, usize), String> {
    let specified = usize::from(start_inset.is_some())
        + usize::from(end_inset.is_some())
        + usize::from(size.is_some());
    if specified != 2 {
        return Err(format!(
            "region {axis_name} must specify exactly two of {start_name}, {end_name}, and {size_name}"
        ));
    }

    let resolved = match (start_inset, end_inset, size) {
        (Some(start), Some(end), None) => {
            let start = (start as usize).min(total);
            let end = total.saturating_sub(end as usize).max(start);
            (start, end)
        }
        (Some(start), None, Some(size)) => {
            let start = (start as usize).min(total);
            let end = start.saturating_add(size as usize).min(total);
            (start, end)
        }
        (None, Some(end), Some(size)) => {
            let end = total.saturating_sub(end as usize);
            let start = end.saturating_sub(size as usize);
            (start, end)
        }
        _ => unreachable!("specified field count was validated above"),
    };

    Ok(resolved)
}

fn crop_line_columns(line: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    let mut result = String::new();
    let mut col = 0;
    for ch in line.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        let next = col + width;
        if next <= start {
            col = next;
            continue;
        }
        if col >= end {
            break;
        }
        if col >= start && next <= end {
            result.push(ch);
        }
        col = next;
    }
    result.trim_end().to_string()
}

pub(crate) fn strip_ansi_sequences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                    continue;
                }
                Some(']') => {
                    chars.next();
                    let mut prev = None;
                    for next in chars.by_ref() {
                        if next == '\u{07}' || (prev == Some('\u{1b}') && next == '\\') {
                            break;
                        }
                        prev = Some(next);
                    }
                    continue;
                }
                _ => continue,
            }
        }
        result.push(ch);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(
        left: Option<u32>,
        right: Option<u32>,
        width: Option<u32>,
        top: Option<u32>,
        bottom: Option<u32>,
        height: Option<u32>,
    ) -> PaneReadRegion {
        PaneReadRegion {
            left,
            right,
            width,
            top,
            bottom,
            height,
        }
    }

    #[test]
    fn crops_first_columns_with_left_and_width() {
        let (text, resolved) = crop_text_region(
            "abcdefghijklmnopqrstuvwxyz",
            &region(Some(0), None, Some(8), Some(0), None, Some(1)),
            Some(26),
            Some(1),
        )
        .unwrap();

        assert_eq!(text, "abcdefgh");
        assert_eq!(resolved.left, 0);
        assert_eq!(resolved.width, 8);
    }

    #[test]
    fn crops_last_columns_with_right_and_width() {
        let (text, resolved) = crop_text_region(
            "abcdefghijklmnopqrstuvwxyz",
            &region(None, Some(0), Some(5), Some(0), None, Some(1)),
            Some(26),
            Some(1),
        )
        .unwrap();

        assert_eq!(text, "vwxyz");
        assert_eq!(resolved.left, 21);
        assert_eq!(resolved.width, 5);
    }

    #[test]
    fn excludes_right_inset_columns() {
        let (text, resolved) = crop_text_region(
            "payload------------------------------------------chrome",
            &region(Some(0), Some(6), None, Some(0), None, Some(1)),
            Some(55),
            Some(1),
        )
        .unwrap();

        assert!(text.starts_with("payload"));
        assert!(!text.contains("chrome"));
        assert_eq!(resolved.width, 49);
    }

    #[test]
    fn crops_rows_with_top_and_height() {
        let (text, resolved) = crop_text_region(
            "row0\nrow1\nrow2\nrow3",
            &region(Some(0), Some(0), None, Some(1), None, Some(2)),
            None,
            None,
        )
        .unwrap();

        assert_eq!(text, "row1\nrow2");
        assert_eq!(resolved.top, 1);
        assert_eq!(resolved.height, 2);
    }

    #[test]
    fn crops_rows_with_bottom_and_height() {
        let (text, resolved) = crop_text_region(
            "row0\nrow1\nrow2\nrow3",
            &region(Some(0), Some(0), None, None, Some(0), Some(2)),
            None,
            None,
        )
        .unwrap();

        assert_eq!(text, "row2\nrow3");
        assert_eq!(resolved.top, 2);
        assert_eq!(resolved.height, 2);
    }

    #[test]
    fn rejects_three_horizontal_fields() {
        let error = crop_text_region(
            "abc",
            &region(Some(0), Some(0), Some(1), Some(0), None, Some(1)),
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            "region x-axis must specify exactly two of left, right, and width"
        );
    }

    #[test]
    fn rejects_missing_vertical_field() {
        let error = crop_text_region(
            "abc",
            &region(Some(0), None, Some(1), Some(0), None, None),
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            "region y-axis must specify exactly two of top, bottom, and height"
        );
    }

    #[test]
    fn strips_ansi_sequences() {
        let stripped = strip_ansi_sequences("\u{1b}[31mpayload\u{1b}[0m");

        assert_eq!(stripped, "payload");
    }
}
