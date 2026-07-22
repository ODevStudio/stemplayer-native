use std::time::Duration;
use stemplayer_domain::{GenerationId, PlayerSnapshot, SourceFrame, TransportState};

#[derive(Clone, Copy, Debug)]
pub struct Playhead {
    frame: SourceFrame,
    end_frame: SourceFrame,
    sample_rate: u32,
    observed_at: Duration,
    generation: GenerationId,
    moving: bool,
}

impl Playhead {
    pub fn from_snapshot(snapshot: &PlayerSnapshot, observed_at: Duration) -> Self {
        let frame = snapshot
            .position()
            .map(|position| position.absolute_frame())
            .unwrap_or(SourceFrame::new(0));
        let source = snapshot.source();
        Self {
            frame,
            end_frame: source
                .map(|value| value.end_frame_exclusive)
                .unwrap_or(frame),
            sample_rate: source.map(|value| value.sample_rate.get()).unwrap_or(1),
            observed_at,
            generation: snapshot.generation(),
            moving: snapshot.transport_state() == TransportState::Playing,
        }
    }

    pub fn update(&mut self, snapshot: &PlayerSnapshot, observed_at: Duration) {
        let next = Self::from_snapshot(snapshot, observed_at);
        let explicit_jump = next.generation != self.generation
            || matches!(
                snapshot.transport_state(),
                TransportState::Seeking | TransportState::Ready | TransportState::Ended
            );
        if explicit_jump || next.frame >= self.visible_frame(observed_at) {
            *self = next;
        } else {
            self.observed_at = observed_at;
            self.moving = next.moving;
            self.end_frame = next.end_frame;
            self.sample_rate = next.sample_rate;
        }
    }

    pub fn visible_frame(self, now: Duration) -> SourceFrame {
        if !self.moving {
            return self.frame;
        }
        let elapsed = now.saturating_sub(self.observed_at).as_nanos();
        let advanced = elapsed
            .saturating_mul(u128::from(self.sample_rate))
            .saturating_div(1_000_000_000)
            .min(i64::MAX as u128) as i64;
        SourceFrame::new(
            self.frame
                .get()
                .saturating_add(advanced)
                .min(self.end_frame.get()),
        )
    }

    pub const fn repaint_needed(self, visible: bool) -> bool {
        visible && self.moving
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemplayer_domain::{
        ChannelCount, DisplayName, Microseconds, PlaybackPosition, PlayerSnapshot, SampleRate,
        SourceKind, SourceMetadata,
    };

    fn snapshot(state: TransportState, frame: i64) -> PlayerSnapshot {
        PlayerSnapshot::new(
            state,
            GenerationId::INITIAL,
            Some(PlaybackPosition::new(
                SourceFrame::new(frame),
                Microseconds::new(frame * 1_000),
            )),
            Some(SourceMetadata {
                display_name: DisplayName::new("demo").unwrap(),
                kind: SourceKind::StudioWavBundle,
                has_master: true,
                sample_rate: SampleRate::new(1_000).unwrap(),
                channel_count: ChannelCount::new(10).unwrap(),
                start_frame: SourceFrame::new(0),
                end_frame_exclusive: SourceFrame::new(10_000),
            }),
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn interpolation_runs_only_while_visible_and_playing() {
        let start = Duration::from_secs(5);
        let mut playhead =
            Playhead::from_snapshot(&snapshot(TransportState::Playing, 1_000), start);
        assert_eq!(
            playhead.visible_frame(start + Duration::from_millis(500)),
            SourceFrame::new(1_500)
        );
        assert!(playhead.repaint_needed(true));
        assert!(!playhead.repaint_needed(false));
        playhead.update(
            &snapshot(TransportState::Paused, 1_500),
            start + Duration::from_millis(500),
        );
        assert!(!playhead.repaint_needed(true));
    }
}
