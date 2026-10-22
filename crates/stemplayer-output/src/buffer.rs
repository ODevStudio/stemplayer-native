use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

pub fn output_refill_interval(
    capacity_frames: usize,
    maximum_render_frames: usize,
    sample_rate: u32,
) -> Result<Duration, PlayerError> {
    if sample_rate == 0 || maximum_render_frames == 0 || maximum_render_frames >= capacity_frames {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Output buffer requires render headroom and a positive sample rate",
        ));
    }
    let headroom = capacity_frames - maximum_render_frames;
    let nanos = (headroom as u128 * 1_000_000_000) / (u128::from(sample_rate) * 2);
    let nanos = u64::try_from(nanos)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Output refill interval is out of range",
            )
        })?;
    Ok(Duration::from_nanos(nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_rate_refill_keeps_render_and_scheduling_headroom() {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000, 384_000] {
            let interval = output_refill_interval(8_192, 2_048, rate).unwrap();
            let consumed = interval.as_nanos() * u128::from(rate) / 1_000_000_000;
            assert!((3_071..=3_072).contains(&consumed));
            assert!(consumed * 2 + 2_048 <= 8_192);
        }
        assert!(output_refill_interval(8_192, 2_048, 384_000).unwrap() < Duration::from_millis(25));
    }

    #[test]
    fn missing_headroom_is_rejected() {
        assert!(output_refill_interval(8_192, 8_192, 48_000).is_err());
        assert!(output_refill_interval(8_192, 0, 48_000).is_err());
        assert!(output_refill_interval(8_192, 2_048, 0).is_err());
    }
}
