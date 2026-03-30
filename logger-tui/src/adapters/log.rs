use logger_runtime::{Band, LogAdapter, Mode, decode_exchange_pairs};

pub fn build_log_display(adapter: &LogAdapter) -> Vec<crate::ui::log_tail::LogRow> {
    adapter
        .ordered_records()
        .into_iter()
        .filter(|r| !r.flags.is_void)
        .enumerate()
        .map(|(i, rec)| {
            let exchange = decode_exchange_pairs(&rec.exchange)
                .unwrap_or_default()
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<_>>()
                .join(" ");
            let band = match rec.band {
                Band::B160m => "160m",
                Band::B80m => "80m",
                Band::B40m => "40m",
                Band::B20m => "20m",
                Band::B15m => "15m",
                Band::B10m => "10m",
                Band::Other => "other",
            };
            let mode = match rec.mode {
                Mode::CW => "CW",
                Mode::SSB => "SSB",
                Mode::Digital => "DIGITAL",
                Mode::Other => "OTHER",
            };
            crate::ui::log_tail::LogRow {
                nr: i as u64 + 1,
                call: rec.callsign_norm.clone(),
                band: band.to_string(),
                mode: mode.to_string(),
                exchange,
            }
        })
        .collect()
}
