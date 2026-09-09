use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CaptionLayout {
    pub title: Range<i32>,
    pub status: Option<(usize, Range<i32>)>,
}

pub(super) fn native_button_reserve(
    window_width: i32,
    client_left: i32,
    client_width: i32,
    frame: i32,
    button_left: i32,
    button_right: i32,
) -> Option<i32> {
    let client_right = client_left.checked_add(client_width)?;
    if button_right <= button_left
        || button_right > window_width
        || button_right < window_width.saturating_sub(frame.saturating_add(2))
        || button_left < client_left
        || button_left >= client_right
    {
        return None;
    }
    Some(client_right - button_left)
}

pub(super) fn caption_width(
    client_width: i32,
    dpi: u32,
    native_reserve: Option<i32>,
    cached: &mut Option<(u32, i32)>,
) -> i32 {
    let reserve = if let Some(reserve) = native_reserve {
        *cached = Some((dpi, reserve));
        reserve
    } else if let Some((old_dpi, reserve)) = *cached {
        (i64::from(reserve) * i64::from(dpi) / i64::from(old_dpi.max(1)))
            .clamp(0, i64::from(i32::MAX)) as i32
    } else {
        (dpi.saturating_mul(160) / 96).min(i32::MAX as u32) as i32
    };
    client_width.saturating_sub(reserve).max(0)
}

/// Use the native per-button horizontal bounds across the visible caption band.
pub(super) fn button_contains(
    button: Range<i32>,
    caption_x: Range<i32>,
    caption_y: Range<i32>,
    point: (i32, i32),
) -> bool {
    (button.start.max(caption_x.start)..button.end.min(caption_x.end)).contains(&point.0)
        && caption_y.contains(&point.1)
}

/// Consume the old caption and invisible resize padding without moving terminal content.
pub(super) fn caption_insets(
    native_top: i32,
    caption_height: i32,
    maximized: bool,
) -> (i32, i32, i32) {
    let native_top = native_top.max(0);
    let frame = native_top - caption_height.clamp(0, native_top);
    // DWM needs zero top inset, including when maximized, to own button hover/hit testing.
    // https://handmade.network/forums/articles/t/9073 ("rect->top += 1" reproduction).
    let text_top = if maximized { frame } else { 0 };
    (0, native_top, text_top)
}

/// Coordinates are caption-client pixels; the desired center is the whole window.
pub(super) fn layout(
    window_width: i32,
    client_left: i32,
    text_left: i32,
    text_right: i32,
    widths: &[i32],
    gap: i32,
) -> CaptionLayout {
    let right = text_right.max(0);
    let left = text_left.clamp(0, right);
    for (index, &width) in widths.iter().enumerate() {
        if width <= 0 || width > right - left {
            continue;
        }
        let desired = (window_width.saturating_sub(width) / 2).saturating_sub(client_left);
        let x = desired.clamp(left, right - width);
        return CaptionLayout {
            title: left..x.saturating_sub(gap.max(0)).max(left),
            status: Some((index, x..x + width)),
        };
    }
    CaptionLayout {
        title: left..right,
        status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_button_bounds_cannot_cover_buttons_after_restore_or_maximize() {
        let mut reserve = None;
        let maximized = native_button_reserve(1788, 8, 1772, 8, 1633, 1779);
        assert_eq!(caption_width(1772, 96, maximized, &mut reserve), 1625);
        let stale = native_button_reserve(1240, 8, 1224, 8, 1633, 1779);
        assert_eq!(stale, None);
        assert_eq!(caption_width(1224, 96, stale, &mut reserve), 1077);
        let stale = native_button_reserve(1788, 8, 1772, 8, 1085, 1231);
        assert_eq!(stale, None);
        assert_eq!(caption_width(1772, 96, stale, &mut reserve), 1625);
    }

    #[test]
    fn unavailable_button_bounds_reserve_space_at_the_current_dpi() {
        assert_eq!(caption_width(1000, 144, None, &mut None), 760);
        assert_eq!(caption_width(1000, 144, None, &mut Some((96, 148))), 778);
        assert_eq!(caption_width(100, 96, None, &mut None), 0);
    }

    #[test]
    fn maximized_buttons_cover_the_visible_top_edge_but_not_the_terminal() {
        // Native TITLEBARINFOEX bounds include the invisible maximize frame.
        assert!(button_contains(1678..1724, 0..1772, 0..23, (1701, 0)));
        assert!(button_contains(1724..1780, 0..1772, 0..23, (1771, 17)));
        assert!(!button_contains(1724..1780, 0..1772, 0..23, (1772, 17)));
        assert!(!button_contains(1724..1780, 0..1772, 0..23, (1750, 23)));
    }

    #[test]
    fn caption_uses_resize_padding_but_content_keeps_its_native_origin() {
        for (native_top, caption, restored, maximized) in [
            (31, 23, (0, 31, 0), (0, 31, 8)),
            (47, 35, (0, 47, 0), (0, 47, 12)),
            (26, 23, (0, 26, 0), (0, 26, 3)),
        ] {
            assert_eq!(caption_insets(native_top, caption, false), restored);
            assert_eq!(caption_insets(native_top, caption, true), maximized);
            assert_eq!(restored.0 + restored.1, native_top);
            assert_eq!(maximized.0 + maximized.1, native_top);
            assert_eq!(maximized.1 - maximized.2, caption);
        }
    }

    #[test]
    fn buttons_limit_space_without_shifting_the_desired_center() {
        for right in [850, 800, 750] {
            let result = layout(1000, 8, 24, right, &[300, 200], 8);
            assert_eq!(result.status, Some((0, 342..642)));
            assert_eq!(result.title, 24..334);
        }
    }

    #[test]
    fn narrow_caption_uses_a_complete_compact_block_or_none() {
        assert_eq!(
            layout(360, 8, 24, 198, &[300, 170], 8).status,
            Some((1, 28..198))
        );
        assert_eq!(layout(200, 8, 24, 100, &[300, 170], 8).status, None);
    }

    #[test]
    fn no_title_or_status_can_overrun_the_drawable_rectangle() {
        for window_width in [0, 120, 300, 800, 1772, 3840] {
            for left in [0, 8, 16] {
                let right = (window_width - 140 - left).max(0);
                let result = layout(window_width, left, 24, right, &[300, 170], 8);
                assert!(result.title.start >= 0 && result.title.end <= right);
                assert!(result.title.start <= result.title.end);
                if let Some((_, range)) = result.status {
                    assert!(range.start >= result.title.end && range.end <= right);
                }
            }
        }
    }

    #[test]
    fn dpi_scaled_layout_keeps_the_same_center() {
        for scale in [1, 2, 3] {
            let result = layout(
                1000 * scale,
                8 * scale,
                24 * scale,
                850 * scale,
                &[300 * scale],
                8 * scale,
            );
            let (_, range) = result.status.unwrap();
            assert_eq!(range.start + range.end + 16 * scale, 1000 * scale);
        }
    }
}
