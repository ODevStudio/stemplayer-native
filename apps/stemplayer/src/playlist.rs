use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use stemplayer_domain::{ErrorCode, PlayerError};

const MAX_PLAYLIST_ITEMS: usize = 512;
const MAX_SCANNED_FILES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaylistItem {
    pub label: String,
    pub source: PathBuf,
}

#[derive(Default)]
pub struct Playlist {
    items: Vec<PlaylistItem>,
    selected: BTreeSet<usize>,
    focus: Option<usize>,
    anchor: Option<usize>,
    active: Option<usize>,
    shuffle: bool,
    repeat: bool,
    remaining: Vec<usize>,
    history: Vec<usize>,
}

impl Playlist {
    pub fn append(&mut self, items: Vec<PlaylistItem>) -> Option<PathBuf> {
        let was_empty = self.items.is_empty();
        let mut seen = self
            .items
            .iter()
            .map(|item| item.source.clone())
            .collect::<BTreeSet<_>>();
        let available = MAX_PLAYLIST_ITEMS.saturating_sub(self.items.len());
        self.items.extend(
            items
                .into_iter()
                .filter(|item| seen.insert(item.source.clone()))
                .take(available),
        );
        if was_empty && !self.items.is_empty() {
            self.select(0, false, false);
            return self.activate(0);
        }
        self.rebuild_shuffle();
        None
    }

    pub fn restore(sources: &[String], selected_source: Option<&str>) -> Self {
        let mut seen = BTreeSet::new();
        let items = sources
            .iter()
            .take(MAX_PLAYLIST_ITEMS)
            .filter_map(|source| {
                let path = PathBuf::from(source);
                let item = discover(std::slice::from_ref(&path))
                    .ok()?
                    .into_iter()
                    .next()?;
                (supported_source(&item.source) && seen.insert(item.source.clone())).then_some(item)
            })
            .collect::<Vec<_>>();
        let focus = selected_source
            .and_then(|selected| {
                items
                    .iter()
                    .position(|item| item.source == Path::new(selected))
            })
            .or_else(|| (!items.is_empty()).then_some(0));
        Self {
            items,
            selected: focus.into_iter().collect(),
            focus,
            anchor: focus,
            active: None,
            ..Self::default()
        }
    }

    pub fn rows(&self) -> Vec<(String, bool)> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.label.clone(), self.selected.contains(&index)))
            .collect()
    }

    pub fn selected(&self) -> Option<usize> {
        self.focus
    }

    pub fn sources(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| item.source.to_str().map(str::to_owned))
            .collect()
    }

    pub fn source_paths(&self) -> Vec<PathBuf> {
        self.items.iter().map(|item| item.source.clone()).collect()
    }

    pub fn selected_source(&self) -> Option<String> {
        self.focus
            .and_then(|index| self.items.get(index))
            .and_then(|item| item.source.to_str())
            .map(str::to_owned)
    }

    pub fn active_source(&self) -> Option<PathBuf> {
        self.active
            .and_then(|index| self.items.get(index))
            .map(|item| item.source.clone())
    }

    pub fn select(&mut self, index: usize, control: bool, shift: bool) -> bool {
        if index >= self.items.len() {
            return false;
        }
        if shift {
            let anchor = self.anchor.or(self.focus).unwrap_or(index);
            if !control {
                self.selected.clear();
            }
            self.selected.extend(anchor.min(index)..=anchor.max(index));
        } else if control {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
            self.anchor = Some(index);
        } else {
            self.selected.clear();
            self.selected.insert(index);
            self.anchor = Some(index);
        }
        self.focus = Some(index);
        true
    }

    pub fn select_context(&mut self, index: usize) -> bool {
        if self.selected.contains(&index) {
            self.focus = Some(index);
            true
        } else {
            self.select(index, false, false)
        }
    }

    pub fn select_all(&mut self) {
        self.selected = (0..self.items.len()).collect();
        self.focus = self.focus.or_else(|| (!self.items.is_empty()).then_some(0));
        self.anchor = self.anchor.or(self.focus);
    }

    pub fn activate(&mut self, index: usize) -> Option<PathBuf> {
        let source = self.activate_index(index)?;
        self.history.clear();
        self.rebuild_shuffle();
        Some(source)
    }

    fn activate_index(&mut self, index: usize) -> Option<PathBuf> {
        let source = self.items.get(index)?.source.clone();
        self.selected.clear();
        self.selected.insert(index);
        self.focus = Some(index);
        self.anchor = Some(index);
        self.active = Some(index);
        Some(source)
    }

    pub fn activate_selected(&mut self) -> Option<PathBuf> {
        self.activate(self.focus?)
    }

    pub fn activate_selected_if_changed(&mut self) -> Option<PathBuf> {
        (self.focus != self.active)
            .then(|| self.activate_selected())
            .flatten()
    }

    pub fn next(&mut self) -> Option<(usize, PathBuf)> {
        self.next_with_repeat(self.repeat)
    }

    pub fn next_after_error(&mut self) -> Option<(usize, PathBuf)> {
        self.next_with_repeat(false)
    }

    fn next_with_repeat(&mut self, repeat: bool) -> Option<(usize, PathBuf)> {
        let active = self.active?;
        let index = if self.shuffle {
            if self.remaining.is_empty() && repeat {
                self.rebuild_shuffle();
            }
            self.remaining
                .pop()
                .or_else(|| (repeat && self.items.len() == 1).then_some(active))?
        } else {
            active
                .checked_add(1)
                .filter(|index| *index < self.items.len())
                .or_else(|| repeat.then_some(0))?
        };
        if index != active {
            self.history.push(active);
        }
        let source = self.activate_index(index)?;
        Some((index, source))
    }

    pub fn previous(&mut self) -> Option<(usize, PathBuf)> {
        let active = self.active?;
        let index = if self.shuffle {
            let index = self.history.pop()?;
            if !self.remaining.contains(&active) {
                self.remaining.push(active);
            }
            index
        } else {
            active
                .checked_sub(1)
                .or_else(|| self.repeat.then(|| self.items.len() - 1))?
        };
        let source = self.activate_index(index)?;
        Some((index, source))
    }

    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle != enabled {
            self.shuffle = enabled;
            self.history.clear();
            self.rebuild_shuffle();
        }
    }

    pub fn set_repeat(&mut self, enabled: bool) {
        self.repeat = enabled;
    }

    fn rebuild_shuffle(&mut self) {
        self.remaining = if self.shuffle {
            (0..self.items.len())
                .filter(|index| Some(*index) != self.active)
                .collect()
        } else {
            Vec::new()
        };
        fastrand::shuffle(&mut self.remaining);
    }

    pub fn remove_selected(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let first = *self.selected.first().unwrap();
        let active_removed = self
            .active
            .is_some_and(|index| self.selected.contains(&index));
        let active = self
            .active
            .map(|index| index.saturating_sub(self.selected.range(..index).count()));
        self.items = self
            .items
            .drain(..)
            .enumerate()
            .filter_map(|(index, item)| (!self.selected.contains(&index)).then_some(item))
            .collect();
        self.selected.clear();
        if active_removed {
            self.focus = None;
            self.anchor = None;
            self.active = None;
        } else {
            let next = (!self.items.is_empty()).then(|| first.min(self.items.len() - 1));
            self.focus = next;
            self.anchor = next;
            self.selected.extend(next);
            self.active = active;
        }
        self.history.clear();
        self.rebuild_shuffle();
        true
    }

    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        let remap = |index| {
            if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        self.selected = self.selected.iter().copied().map(remap).collect();
        self.focus = self.focus.map(remap);
        self.anchor = self.anchor.map(remap);
        self.active = self.active.map(remap);
        self.history.clear();
        self.rebuild_shuffle();
        true
    }
}
pub fn discover(inputs: &[PathBuf]) -> Result<Vec<PlaylistItem>, PlayerError> {
    if inputs.len() > MAX_PLAYLIST_ITEMS {
        return Err(error("Too many dropped playlist entries"));
    }
    let mut candidates = BTreeSet::new();
    for input in inputs {
        let metadata = fs::symlink_metadata(input)
            .map_err(|source| error("Dropped source is unavailable").with_source(source))?;
        if metadata.file_type().is_symlink() {
            return Err(error("Dropped sources cannot be symbolic links"));
        }
        if metadata.is_dir() {
            collect_directory(input, None, &mut candidates)?;
        } else if metadata.is_file() {
            candidates.insert(input.clone());
            if let Some(prefix) = wav_prefix(input)
                && let Some(parent) = input.parent()
            {
                collect_directory(parent, Some(&prefix), &mut candidates)?;
            }
        }
    }

    let mut items = Vec::new();
    let mut wav_groups = BTreeMap::<(PathBuf, String), WavGroup>::new();
    let mut manifest_files = BTreeSet::new();
    for path in candidates {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches_extension(extension, &["m4a", "mp4", "mp3", "flac", "ogg", "oga"]) {
            items.push(PlaylistItem {
                label: file_label(&path),
                source: path,
            });
        } else if extension.eq_ignore_ascii_case("json") {
            if let Some(manifest) = valid_manifest(&path) {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                manifest_files.extend(
                    manifest
                        .tracks
                        .into_iter()
                        .map(|track| (parent.to_path_buf(), track.filename.to_ascii_lowercase())),
                );
                items.push(PlaylistItem {
                    label: file_label(&path),
                    source: path,
                });
            }
        } else if let Some(prefix) = wav_prefix(&path) {
            let Some(role) = wav_role(&path) else {
                continue;
            };
            let parent = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let group = wav_groups
                .entry((parent, prefix.clone()))
                .or_insert_with(|| WavGroup::new(prefix));
            group.add(role, path);
        } else if extension.eq_ignore_ascii_case("wav") {
            items.push(PlaylistItem {
                label: file_label(&path),
                source: path,
            });
        }
    }
    items.extend(wav_groups.into_iter().filter_map(|((parent, _), group)| {
        if !group.complete()
            || group.paths.iter().flatten().any(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        manifest_files.contains(&(parent.clone(), name.to_ascii_lowercase()))
                    })
            })
        {
            return None;
        }
        Some(PlaylistItem {
            label: if group.prefix.is_empty() {
                parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("WAV bundle")
                    .to_owned()
            } else {
                group.prefix
            },
            source: group.paths.into_iter().flatten().next()?,
        })
    }));
    items.sort_by(|left, right| {
        left.label
            .to_ascii_lowercase()
            .cmp(&right.label.to_ascii_lowercase())
            .then_with(|| left.source.cmp(&right.source))
    });
    items.truncate(MAX_PLAYLIST_ITEMS);
    Ok(items)
}

struct WavGroup {
    prefix: String,
    paths: [Option<PathBuf>; 5],
}

impl WavGroup {
    fn new(prefix: String) -> Self {
        Self {
            prefix,
            paths: std::array::from_fn(|_| None),
        }
    }

    fn add(&mut self, role: usize, path: PathBuf) {
        if self.paths[role].is_none() {
            self.paths[role] = Some(path);
        }
    }

    fn complete(&self) -> bool {
        self.paths[..4].iter().all(Option::is_some)
    }
}

fn collect_directory(
    directory: &Path,
    wav_prefix_filter: Option<&str>,
    candidates: &mut BTreeSet<PathBuf>,
) -> Result<(), PlayerError> {
    for (index, entry) in fs::read_dir(directory)
        .map_err(|source| error("Playlist folder cannot be read").with_source(source))?
        .enumerate()
    {
        if index >= MAX_SCANNED_FILES || candidates.len() >= MAX_SCANNED_FILES {
            return Err(error("Playlist folder contains too many entries"));
        }
        let entry =
            entry.map_err(|source| error("Playlist entry cannot be read").with_source(source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| error("Playlist entry cannot be inspected").with_source(source))?;
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if wav_prefix_filter.is_none_or(|prefix| {
            wav_prefix(&path).is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        }) {
            candidates.insert(path);
        }
    }
    Ok(())
}

fn supported_source(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    match extension.as_str() {
        "mp4" => stemplayer_decode::inspect_compressed_container(path, &cancelled).is_ok(),
        "m4a" => {
            stemplayer_decode::inspect_compressed_container(path, &cancelled).is_ok()
                || stemplayer_decode::inspect_audio_file(path).is_ok()
        }
        "mp3" | "flac" | "ogg" | "oga" => stemplayer_decode::inspect_audio_file(path).is_ok(),
        "wav" if wav_prefix(path).is_none() => stemplayer_decode::inspect_audio_file(path).is_ok(),
        "json" | "wav" => stemplayer_platform::select_local_source(path)
            .and_then(|group| stemplayer_format::StudioBundle::inspect(group, false, &cancelled))
            .is_ok(),
        _ => false,
    }
}

fn valid_manifest(path: &Path) -> Option<stemplayer_format::StudioManifest> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > stemplayer_format::MAX_MANIFEST_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)
        .ok()?
        .take(stemplayer_format::MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    stemplayer_format::parse_manifest(&bytes).ok()
}

fn wav_prefix(path: &Path) -> Option<String> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let (prefix, role) = stem.rsplit_once('.').unwrap_or(("", stem));
    wav_role_name(role).map(|_| prefix.to_owned())
}

fn wav_role(path: &Path) -> Option<usize> {
    let stem = path.file_stem()?.to_str()?;
    wav_role_name(stem.rsplit('.').next()?)
}

fn wav_role_name(role: &str) -> Option<usize> {
    ["drums", "bass", "other", "vocals", "master"]
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(role))
}

fn file_label(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Track")
        .to_owned()
}

fn matches_extension(extension: &str, supported: &[&str]) -> bool {
    supported
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::ManifestInvalid, summary)
        .with_user_action("Choose supported stem files")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_containers_manifests_and_role_suffixed_wav_bundles() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-playlist-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("first.stem.mp4"), []).unwrap();
        for role in ["drums", "bass", "other", "vocals"] {
            fs::write(root.join(format!("second.{role}.wav")), []).unwrap();
        }
        let items = discover(std::slice::from_ref(&root)).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["first.stem", "second"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn playlist_scan_limit_counts_unsupported_entries() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-playlist-limit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for index in 0..=MAX_SCANNED_FILES {
            fs::create_dir(root.join(index.to_string())).unwrap();
        }

        let result = discover(std::slice::from_ref(&root));

        fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn active_selection_is_not_reopened() {
        let mut playlist = Playlist::default();
        playlist.append(vec![PlaylistItem {
            label: "Track".to_owned(),
            source: PathBuf::from("track.stem.m4a"),
        }]);

        assert_eq!(
            playlist.activate_selected(),
            Some(PathBuf::from("track.stem.m4a"))
        );
        assert_eq!(playlist.activate_selected_if_changed(), None);
    }

    #[test]
    fn playlist_navigation_supports_previous_repeat_and_shuffle() {
        let mut playlist = Playlist::default();
        playlist.append((0..4).map(item).collect());
        playlist.activate(1);

        assert_eq!(playlist.previous(), Some((0, item(0).source)));
        assert_eq!(playlist.previous(), None);
        playlist.set_repeat(true);
        assert_eq!(playlist.previous(), Some((3, item(3).source)));
        assert_eq!(playlist.next(), Some((0, item(0).source)));
        playlist.activate(3);
        assert_eq!(playlist.next_after_error(), None);
        playlist.activate(0);

        playlist.set_repeat(false);
        playlist.set_shuffle(true);
        let shuffled = (0..3)
            .map(|_| playlist.next().unwrap().0)
            .collect::<BTreeSet<_>>();
        assert_eq!(shuffled, BTreeSet::from([1, 2, 3]));
        assert_eq!(playlist.next(), None);
    }

    fn item(index: usize) -> PlaylistItem {
        PlaylistItem {
            label: format!("Track {index}"),
            source: PathBuf::from(format!("track-{index}.mp3")),
        }
    }

    #[test]
    fn append_preserves_items_deduplicates_limits_and_keeps_active() {
        let mut playlist = Playlist::default();
        assert_eq!(
            playlist.append(vec![item(0), item(1)]),
            Some(item(0).source)
        );
        assert_eq!(playlist.activate(1), Some(item(1).source));
        let added = (1..600).map(item).collect();

        assert_eq!(playlist.append(added), None);
        assert_eq!(playlist.items.len(), MAX_PLAYLIST_ITEMS);
        assert_eq!(playlist.items[0], item(0));
        assert_eq!(playlist.items[1], item(1));
        assert_eq!(playlist.active, Some(1));
        assert_eq!(playlist.selected(), Some(1));
    }

    #[test]
    fn selection_supports_click_control_shift_and_select_all() {
        let mut playlist = Playlist::default();
        playlist.append((0..6).map(item).collect());

        playlist.select(1, false, false);
        assert_eq!(playlist.selected, BTreeSet::from([1]));
        playlist.select(3, true, false);
        assert_eq!(playlist.selected, BTreeSet::from([1, 3]));
        playlist.select(5, false, true);
        assert_eq!(playlist.selected, BTreeSet::from([3, 4, 5]));
        playlist.select(1, true, true);
        assert_eq!(playlist.selected, BTreeSet::from([1, 2, 3, 4, 5]));
        playlist.select(3, true, false);
        assert_eq!(playlist.selected, BTreeSet::from([1, 2, 4, 5]));
        playlist.select_all();
        assert_eq!(playlist.selected, BTreeSet::from([0, 1, 2, 3, 4, 5]));
    }

    #[test]
    fn context_selection_preserves_selected_rows() {
        let mut playlist = Playlist::default();
        playlist.append((0..4).map(item).collect());
        playlist.select(1, false, false);
        playlist.select(2, true, false);

        playlist.select_context(1);
        assert_eq!(playlist.selected, BTreeSet::from([1, 2]));
        playlist.select_context(3);
        assert_eq!(playlist.selected, BTreeSet::from([3]));
    }

    #[test]
    fn removal_reselects_the_next_or_previous_last_item() {
        let mut playlist = Playlist::default();
        playlist.append((0..5).map(item).collect());
        playlist.activate(4);
        playlist.select(0, false, false);
        assert!(playlist.remove_selected());
        assert_eq!(playlist.selected(), Some(0));
        assert_eq!(playlist.active, Some(3));

        playlist.select(1, false, false);
        assert!(playlist.remove_selected());
        assert_eq!(playlist.selected(), Some(1));
        assert_eq!(playlist.active, Some(2));

        playlist.activate(0);
        playlist.select(2, false, false);
        assert!(playlist.remove_selected());
        assert_eq!(playlist.selected(), Some(1));
        assert_eq!(playlist.active, Some(0));
    }

    #[test]
    fn removing_all_items_clears_playlist() {
        let mut playlist = Playlist::default();
        playlist.append((0..3).map(item).collect());
        playlist.active = None;
        playlist.select_all();

        assert!(playlist.remove_selected());
        assert!(playlist.items.is_empty());
        assert_eq!(playlist.selected(), None);
        assert_eq!(playlist.next(), None);
    }

    #[test]
    fn removing_active_item_disables_automatic_advance() {
        let mut playlist = Playlist::default();
        playlist.append((0..4).map(item).collect());
        playlist.activate(1);
        playlist.select(1, false, false);
        playlist.select(2, true, false);

        assert!(playlist.remove_selected());
        assert_eq!(playlist.sources(), vec!["track-0.mp3", "track-3.mp3"]);
        assert_eq!(playlist.active, None);
        assert_eq!(playlist.selected(), None);
        assert_eq!(playlist.next(), None);
    }

    #[test]
    fn moving_items_preserves_selection_focus_and_active_source() {
        let mut playlist = Playlist::default();
        playlist.append((0..5).map(item).collect());
        playlist.activate(1);
        playlist.select(3, true, false);

        assert!(playlist.move_item(1, 4));
        assert_eq!(
            playlist.sources(),
            vec![
                "track-0.mp3",
                "track-2.mp3",
                "track-3.mp3",
                "track-4.mp3",
                "track-1.mp3",
            ]
        );
        assert_eq!(playlist.selected, BTreeSet::from([2, 4]));
        assert_eq!(playlist.focus, Some(2));
        assert_eq!(playlist.anchor, Some(2));
        assert_eq!(playlist.active, Some(4));
        assert!(!playlist.move_item(4, 4));
        assert!(!playlist.move_item(8, 0));
    }
    #[test]
    fn discovers_ordinary_formats_without_promoting_incomplete_wav_stems() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-audio-playlist-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for name in [
            "one.mp3",
            "two.flac",
            "three.ogg",
            "four.oga",
            "five.wav",
            "incomplete.drums.wav",
        ] {
            fs::write(root.join(name), []).unwrap();
        }
        let items = discover(std::slice::from_ref(&root)).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["five", "four", "one", "three", "two"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_preserves_order_deduplicates_and_falls_back() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-playlist-restore-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let first = root.join("first.mp3");
        let second = root.join("second.flac");
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/stemplayer-decode/tests/fixtures");
        fs::copy(fixture_root.join("stereo.mp3"), &first).unwrap();
        fs::copy(fixture_root.join("stereo.flac"), &second).unwrap();
        let corrupt = root.join("corrupt.ogg");
        fs::write(&corrupt, b"not audio").unwrap();
        let sources = vec![
            second.to_string_lossy().into_owned(),
            root.join("missing.ogg").to_string_lossy().into_owned(),
            corrupt.to_string_lossy().into_owned(),
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        let restored = Playlist::restore(&sources, Some(first.to_string_lossy().as_ref()));
        assert_eq!(
            restored.sources(),
            vec![sources[0].clone(), sources[3].clone()]
        );
        assert_eq!(restored.selected(), Some(1));

        let fallback = Playlist::restore(&sources, Some("missing"));
        assert_eq!(fallback.selected(), Some(0));
        fs::remove_dir_all(root).unwrap();
    }
}
