pub trait OutputSample: Copy {
    const CLIPS: bool = false;

    fn silence() -> Self;
    fn from_float(value: f32) -> Self;
}

impl OutputSample for f32 {
    fn silence() -> Self {
        0.0
    }

    fn from_float(value: f32) -> Self {
        value
    }
}

impl OutputSample for f64 {
    fn silence() -> Self {
        0.0
    }

    fn from_float(value: f32) -> Self {
        f64::from(value)
    }
}

impl OutputSample for i16 {
    const CLIPS: bool = true;

    fn silence() -> Self {
        0
    }

    fn from_float(value: f32) -> Self {
        if !value.is_finite() {
            Self::silence()
        } else if value <= -1.0 {
            Self::MIN
        } else if value >= 1.0 {
            Self::MAX
        } else {
            (value * Self::MAX as f32).round() as Self
        }
    }
}

impl OutputSample for u16 {
    const CLIPS: bool = true;

    fn silence() -> Self {
        Self::MAX / 2 + 1
    }

    fn from_float(value: f32) -> Self {
        if value.is_finite() {
            (((value.clamp(-1.0, 1.0) + 1.0) * 0.5) * Self::MAX as f32).round() as Self
        } else {
            Self::silence()
        }
    }
}

impl OutputSample for i32 {
    const CLIPS: bool = true;

    fn silence() -> Self {
        0
    }

    fn from_float(value: f32) -> Self {
        if !value.is_finite() {
            Self::silence()
        } else if value <= -1.0 {
            Self::MIN
        } else if value >= 1.0 {
            Self::MAX
        } else {
            (f64::from(value) * f64::from(Self::MAX)).round() as Self
        }
    }
}

impl OutputSample for u32 {
    const CLIPS: bool = true;

    fn silence() -> Self {
        Self::MAX / 2 + 1
    }

    fn from_float(value: f32) -> Self {
        if value.is_finite() {
            (((f64::from(value.clamp(-1.0, 1.0)) + 1.0) * 0.5) * f64::from(Self::MAX)).round()
                as Self
        } else {
            Self::silence()
        }
    }
}
