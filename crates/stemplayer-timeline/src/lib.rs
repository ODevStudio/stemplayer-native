#![deny(unsafe_code)]

use stemplayer_domain::{
    ErrorCode, FrameCount, Microseconds, PlayerError, SampleRate, SourceFrame,
};

pub fn frames_to_microseconds(
    frame: SourceFrame,
    sample_rate: SampleRate,
) -> Result<Microseconds, PlayerError> {
    scale_nonnegative(frame.get(), 1_000_000, sample_rate.get()).map(Microseconds::new)
}

pub fn microseconds_to_frames(
    time: Microseconds,
    sample_rate: SampleRate,
) -> Result<SourceFrame, PlayerError> {
    scale_nonnegative(time.get(), sample_rate.get(), 1_000_000).map(SourceFrame::new)
}

pub fn checked_duration(
    start: SourceFrame,
    end_frame_exclusive: SourceFrame,
) -> Result<FrameCount, PlayerError> {
    let difference = end_frame_exclusive
        .get()
        .checked_sub(start.get())
        .filter(|value| *value >= 0)
        .ok_or_else(invalid_timeline)?;
    FrameCount::new(difference as u64)
}

fn scale_nonnegative(value: i64, numerator: u32, denominator: u32) -> Result<i64, PlayerError> {
    if value < 0 || denominator == 0 {
        return Err(invalid_timeline());
    }
    let result = i128::from(value)
        .checked_mul(i128::from(numerator))
        .ok_or_else(invalid_timeline)?
        / i128::from(denominator);
    i64::try_from(result).map_err(|_| invalid_timeline())
}

fn invalid_timeline() -> PlayerError {
    PlayerError::new(
        ErrorCode::TimelineInvalid,
        "Timeline value cannot be represented exactly within its bounds",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_conversion_floors_without_accumulating_float_error() {
        let rate = SampleRate::new(44_100).unwrap();
        assert_eq!(
            frames_to_microseconds(SourceFrame::new(1), rate)
                .unwrap()
                .get(),
            22
        );
        assert_eq!(
            microseconds_to_frames(Microseconds::new(22), rate)
                .unwrap()
                .get(),
            0
        );
        assert_eq!(
            frames_to_microseconds(SourceFrame::new(44_100), rate)
                .unwrap()
                .get(),
            1_000_000
        );
    }

    #[test]
    fn invalid_and_overflowing_values_fail() {
        let rate = SampleRate::new(1).unwrap();
        assert!(frames_to_microseconds(SourceFrame::new(-1), rate).is_err());
        assert!(frames_to_microseconds(SourceFrame::new(i64::MAX), rate).is_err());
        assert!(microseconds_to_frames(Microseconds::new(-1), rate).is_err());
        assert!(checked_duration(SourceFrame::new(5), SourceFrame::new(4)).is_err());
    }

    #[test]
    fn half_open_duration_is_exact() {
        assert_eq!(
            checked_duration(SourceFrame::new(-10), SourceFrame::new(10))
                .unwrap()
                .get(),
            20
        );
    }
}
