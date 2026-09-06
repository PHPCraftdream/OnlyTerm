/// Convert high-resolution wheel units without wrapping the signed delta.
pub(super) fn lines(delta: i16, speed: i16, remainder: &mut i16) -> i16 {
    let scaled = i32::from(delta) * i32::from(speed);
    if scaled == 0 {
        return 0;
    }
    if i32::from(*remainder).signum() != scaled.signum() {
        *remainder = 0;
    }
    let total = scaled + i32::from(*remainder);
    *remainder = (total % 120) as i16;
    // Keep negation safe for consumers of the signed i16 wheel event.
    (total / 120).clamp(-i32::from(i16::MAX), i32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::lines;

    #[test]
    fn large_delta_does_not_reverse_scroll_direction() {
        let mut remainder = 0;
        assert_eq!(lines(12000, 3, &mut remainder), 300);
        assert_eq!(lines(-12000, 3, &mut remainder), -300);
        assert_eq!(lines(i16::MIN, i16::MAX, &mut remainder), -i16::MAX);
        assert_eq!(lines(i16::MAX, i16::MAX, &mut remainder), i16::MAX);
    }

    #[test]
    fn partial_ticks_survive_full_ticks_and_reset_on_direction_change() {
        let mut remainder = 0;
        assert_eq!(lines(1, 3, &mut remainder), 0);
        assert_eq!(lines(120, 3, &mut remainder), 3);
        assert_eq!(remainder, 3);
        assert_eq!(lines(39, 3, &mut remainder), 1);
        assert_eq!(remainder, 0);
        assert_eq!(lines(1, 3, &mut remainder), 0);
        assert_eq!(lines(-40, 3, &mut remainder), -1);
        assert_eq!(remainder, 0);
    }
}
