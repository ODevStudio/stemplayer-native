use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StemRole {
    Drums,
    Bass,
    Other,
    Vocals,
}

impl StemRole {
    pub const ALL: [Self; 4] = [Self::Drums, Self::Bass, Self::Other, Self::Vocals];

    pub const fn index(self) -> usize {
        match self {
            Self::Drums => 0,
            Self::Bass => 1,
            Self::Other => 2,
            Self::Vocals => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackRole {
    Master,
    Stem(StemRole),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    MasterReference,
    StemMix,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputPreference {
    #[default]
    Automatic,
    MasterReference,
    StemMix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    StudioWavBundle,
    LocalStemContainer,
    LocalAudioFile,
    HlsStemset,
}
