pub mod registry;
pub mod spec_driven;
pub mod traits;

use traits::ContestEntry;

pub fn contest_from_id(id: &str) -> Option<Box<dyn ContestEntry>> {
    if let Some(meta) = registry::find_spec_contest(id) {
        return spec_driven::SpecDrivenContest::new(meta)
            .map(|c| Box::new(c) as Box<dyn ContestEntry>);
    }
    None
}

/// Normalize an arbitrary rig mode string to one of the four canonical
/// `&'static str` values used throughout clogger: `"CW"`, `"SSB"`,
/// `"DIGITAL"`, or `"OTHER"`. Case-insensitive, trims whitespace. Returns
/// a static slice — no allocation, safe to use as a map key or cache tag.
pub fn normalize_mode(mode: &str) -> &'static str {
    let trimmed = mode.trim();
    if trimmed.eq_ignore_ascii_case("CW") {
        "CW"
    } else if trimmed.eq_ignore_ascii_case("SSB") {
        "SSB"
    } else if trimmed.eq_ignore_ascii_case("DIGITAL") {
        "DIGITAL"
    } else {
        "OTHER"
    }
}

pub fn freq_to_band_label(freq_hz: u64) -> &'static str {
    match freq_hz {
        1_800_000..=2_000_000 => "160m",
        3_500_000..=4_000_000 => "80m",
        7_000_000..=7_300_000 => "40m",
        14_000_000..=14_350_000 => "20m",
        21_000_000..=21_450_000 => "15m",
        28_000_000..=29_700_000 => "10m",
        _ => "other",
    }
}

pub fn filtered_bandmap_spots(spots: &[crate::state::Spot], band: &str, mode: &str) -> Vec<crate::state::Spot> {
    let (min, max) = band_freq_range(band);
    let mut out: Vec<_> = spots
        .iter()
        .filter(|s| s.freq_hz >= min && s.freq_hz <= max && s.mode == mode)
        .cloned()
        .collect();
    out.sort_by_key(|s| s.freq_hz);
    out.dedup_by(|a, b| a.call == b.call);
    out
}

pub fn band_freq_range(band: &str) -> (u64, u64) {
    match band {
        "160m" => (1_800_000, 2_000_000),
        "80m" => (3_500_000, 4_000_000),
        "40m" => (7_000_000, 7_300_000),
        "20m" => (14_000_000, 14_350_000),
        "15m" => (21_000_000, 21_450_000),
        "10m" => (28_000_000, 29_700_000),
        _ => (0, 0),
    }
}

/// A small cache of `filtered_bandmap_spots` results, keyed by (band, mode).
///
/// The cache is invalidated by a monotonic version counter on `AppState`
/// (`bandmap_version`) that bumps whenever the bandmap is mutated. Readers
/// pass the current version into `get_or_build`; if it differs from the
/// stored version, all entries are cleared and rebuilt on demand.
///
/// Designed for the contest TUI's two hot consumers:
///
/// - `bandmap.rs::render` (20 Hz × ≤2 radios = up to 40 lookups/sec)
/// - `analytics::compute_worked_calls` and `compute_avail` (run on every
///   text keystroke and spot event; `compute_avail` looks up 6 bands)
///
/// Keys are `&'static str` — callers normalize band via
/// `freq_to_band_label` (returns `&'static str`) and mode via
/// `normalize_mode` (returns `&'static str`). This avoids allocating a
/// String per lookup.
///
/// Entry count is bounded by the distinct (band, mode) pairs the active
/// session actually looks up — in practice ≤ 12 (6 bands × at most 2
/// modes in SO2R). Linear scan is fine at this size.
#[derive(Default)]
pub struct BandmapCache {
    version: u64,
    entries: Vec<BandmapCacheEntry>,
}

struct BandmapCacheEntry {
    band: &'static str,
    mode: &'static str,
    spots: Vec<crate::state::Spot>,
}

impl BandmapCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch filtered spots for `(band, mode)`, rebuilding if the version
    /// has advanced or this key is new. Returns a borrowed slice whose
    /// lifetime is tied to `&mut self`.
    pub fn get_or_build(
        &mut self,
        bandmap: &[crate::state::Spot],
        bandmap_version: u64,
        band: &'static str,
        mode: &'static str,
    ) -> &[crate::state::Spot] {
        if self.version != bandmap_version {
            self.entries.clear();
            self.version = bandmap_version;
        }
        let idx = match self
            .entries
            .iter()
            .position(|e| e.band == band && e.mode == mode)
        {
            Some(i) => i,
            None => {
                let spots = filtered_bandmap_spots(bandmap, band, mode);
                self.entries.push(BandmapCacheEntry { band, mode, spots });
                self.entries.len() - 1
            }
        };
        &self.entries[idx].spots
    }
}
