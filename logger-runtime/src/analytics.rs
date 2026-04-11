use std::collections::HashSet;

use logger_core::contest::{BandmapCache, band_freq_range, freq_to_band_label, normalize_mode};
use logger_core::{DupeChecker, MultChecker, Spot};

use crate::log_adapter::LogAdapter;
use crate::scoring::BAND_LABELS;

#[derive(Default)]
pub struct AvailSummary {
    pub by_band: Vec<(String, u32, u32)>, // (band_label, unworked_qsos, new_mults)
    pub total_qsos: u32,
    pub total_mults: u32,
}

#[derive(Default)]
pub struct RateInfo {
    pub last_10_minutes: Option<f64>,
    pub last_100_minutes: Option<f64>,
    pub rate_per_hour: u32,
}

#[derive(Default)]
pub struct WorkedCalls {
    pub worked: HashSet<String>,
    pub mults: HashSet<String>,
}

pub fn compute_worked_calls(
    bandmap: &[Spot],
    bandmap_version: u64,
    bandmap_cache: &mut BandmapCache,
    freq_hz: u64,
    mode: &str,
    log_adapter: &LogAdapter,
) -> WorkedCalls {
    let mut result = WorkedCalls::default();
    if freq_hz == 0 {
        return result;
    }
    let band = freq_to_band_label(freq_hz);
    let mode = normalize_mode(mode);
    // Iterate through the cached filter result — clone strings out
    // individually to avoid holding the cache borrow past the loop.
    let spots = bandmap_cache.get_or_build(bandmap, bandmap_version, band, mode);
    for spot in spots {
        let call_norm = spot.call.to_ascii_uppercase();
        if log_adapter.is_dupe(&call_norm, band, mode) {
            result.worked.insert(spot.call.clone());
        } else if log_adapter.is_new_mult(&call_norm, band, mode) {
            result.mults.insert(spot.call.clone());
        }
    }
    result
}

pub fn compute_avail(
    bandmap: &[Spot],
    bandmap_version: u64,
    bandmap_cache: &mut BandmapCache,
    mode: &str,
    log_adapter: &LogAdapter,
) -> AvailSummary {
    let mut by_band = Vec::new();
    let mut total_qsos = 0u32;
    let mut total_mults = 0u32;
    let mode = normalize_mode(mode);
    for &band in BAND_LABELS {
        let (min, _max) = band_freq_range(band);
        if min == 0 {
            continue;
        }
        let spots = bandmap_cache.get_or_build(bandmap, bandmap_version, band, mode);
        let mut qsos = 0u32;
        let mut mults = 0u32;
        for s in spots {
            let call_norm = s.call.to_ascii_uppercase();
            if !log_adapter.is_dupe(&call_norm, band, mode) {
                qsos += 1;
                if log_adapter.is_new_mult(&call_norm, band, mode) {
                    mults += 1;
                }
            }
        }
        if qsos > 0 {
            by_band.push((band.to_string(), qsos, mults));
            total_qsos += qsos;
            total_mults += mults;
        }
    }
    AvailSummary { by_band, total_qsos, total_mults }
}

pub fn compute_rate(log_adapter: &LogAdapter, now_ms: u64) -> RateInfo {
    let records = log_adapter.ordered_records();
    let timestamps: Vec<u64> = records
        .iter()
        .filter(|r| !r.flags.is_void)
        .map(|r| r.ts_ms)
        .collect();

    let count = timestamps.len();

    let last_10_minutes = if count >= 10 {
        let t = timestamps[count - 10];
        Some((now_ms.saturating_sub(t)) as f64 / 60_000.0)
    } else {
        None
    };

    let last_100_minutes = if count >= 100 {
        let t = timestamps[count - 100];
        Some((now_ms.saturating_sub(t)) as f64 / 60_000.0)
    } else {
        None
    };

    let rate_per_hour = match last_10_minutes {
        Some(mins) if mins > 0.0 => (10.0 / mins * 60.0) as u32,
        _ => 0,
    };

    RateInfo {
        last_10_minutes,
        last_100_minutes,
        rate_per_hour,
    }
}
