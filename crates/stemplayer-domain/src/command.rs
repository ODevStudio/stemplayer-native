use crate::{
    BoundedText, CommandId, DeviceId, GainDb, Microseconds, OutputPreference, SourceFrame, StemRole,
};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLocator {
    LocalPath(BoundedText<1024>),
    HlsUrl(BoundedText<2048>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheCommand {
    Clear,
    Enable,
    Disable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerCommand {
    Open(SourceLocator),
    Unload,
    Play,
    Pause,
    Stop,
    SeekFrame(SourceFrame),
    SeekTime(Microseconds),
    SetGain { role: StemRole, gain_db: GainDb },
    SetMute { role: StemRole, muted: bool },
    SetSolo { role: StemRole, solo: bool },
    ResetMix,
    SetOutputPreference(OutputPreference),
    SelectDevice(Option<DeviceId>),
    Cache(CacheCommand),
    Cancel(CommandId),
    Shutdown,
}
