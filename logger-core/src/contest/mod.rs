pub mod cqww;
pub mod cwt;
pub mod sweeps;
pub mod traits;

use traits::ContestEntry;

pub fn contest_from_id(id: &str) -> Option<Box<dyn ContestEntry>> {
    match id.to_ascii_lowercase().as_str() {
        "cqww" => Some(Box::new(cqww::CqwwContest::default())),
        "cwt" => Some(Box::new(cwt::CwtContest::default())),
        "sweeps" => Some(Box::new(sweeps::SweepsContest)),
        _ => None,
    }
}

pub fn freq_to_band_label(freq_hz: u64) -> String {
    match freq_hz {
        1_800_000..=2_000_000 => "160m",
        3_500_000..=4_000_000 => "80m",
        7_000_000..=7_300_000 => "40m",
        14_000_000..=14_350_000 => "20m",
        21_000_000..=21_450_000 => "15m",
        28_000_000..=29_700_000 => "10m",
        _ => "other",
    }
    .to_string()
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
