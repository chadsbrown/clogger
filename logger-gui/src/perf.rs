//! Lightweight process/system performance sampling for the GUI.
//!
//! This intentionally uses Linux `/proc` directly instead of pulling in a
//! metrics crate. The pane is diagnostic, so a best-effort snapshot with clear
//! fallback errors is preferable to another runtime dependency.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PerfSnapshot {
    pub process_cpu_percent: Option<f32>,
    pub system_cpu_percent: Option<f32>,
    pub rss_bytes: Option<u64>,
    pub virtual_bytes: Option<u64>,
    pub system_memory_used_percent: Option<f32>,
    pub system_memory_used_bytes: Option<u64>,
    pub system_memory_total_bytes: Option<u64>,
    pub thread_count: Option<u64>,
    pub fd_count: Option<u64>,
    pub uptime: Duration,
    pub sample_count: u64,
    pub error: Option<String>,
}

impl Default for PerfSnapshot {
    fn default() -> Self {
        Self {
            process_cpu_percent: None,
            system_cpu_percent: None,
            rss_bytes: None,
            virtual_bytes: None,
            system_memory_used_percent: None,
            system_memory_used_bytes: None,
            system_memory_total_bytes: None,
            thread_count: None,
            fd_count: None,
            uptime: Duration::ZERO,
            sample_count: 0,
            error: None,
        }
    }
}

pub struct PerfSampler {
    started: Instant,
    previous: Option<RawSample>,
    snapshot: PerfSnapshot,
}

impl PerfSampler {
    pub fn new() -> Self {
        let mut sampler = Self {
            started: Instant::now(),
            previous: None,
            snapshot: PerfSnapshot::default(),
        };
        sampler.sample();
        sampler
    }

    pub fn sample(&mut self) {
        let uptime = self.started.elapsed();
        let sample_count = self.snapshot.sample_count.saturating_add(1);

        match read_raw_sample() {
            Ok(raw) => {
                let (process_cpu_percent, system_cpu_percent) = self
                    .previous
                    .as_ref()
                    .map(|previous| cpu_percentages(previous, &raw))
                    .unwrap_or((None, None));

                self.snapshot = PerfSnapshot {
                    process_cpu_percent,
                    system_cpu_percent,
                    rss_bytes: raw.process.rss_bytes,
                    virtual_bytes: raw.process.virtual_bytes,
                    system_memory_used_percent: raw.system_memory.used_percent,
                    system_memory_used_bytes: raw.system_memory.used_bytes,
                    system_memory_total_bytes: raw.system_memory.total_bytes,
                    thread_count: raw.process.thread_count,
                    fd_count: raw.process.fd_count,
                    uptime,
                    sample_count,
                    error: None,
                };
                self.previous = Some(raw);
            }
            Err(error) => {
                self.snapshot.uptime = uptime;
                self.snapshot.sample_count = sample_count;
                self.snapshot.error = Some(error);
            }
        }
    }

    pub fn snapshot(&self) -> &PerfSnapshot {
        &self.snapshot
    }
}

fn cpu_percentages(previous: &RawSample, current: &RawSample) -> (Option<f32>, Option<f32>) {
    let delta_total = current
        .total_cpu_ticks
        .saturating_sub(previous.total_cpu_ticks);
    if delta_total == 0 {
        return (None, None);
    }

    let delta_process = current
        .process_cpu_ticks
        .saturating_sub(previous.process_cpu_ticks);
    let process =
        Some((delta_process as f32 / delta_total as f32) * current.cpu_count.max(1) as f32 * 100.0);

    let delta_idle = current
        .idle_cpu_ticks
        .saturating_sub(previous.idle_cpu_ticks);
    let busy = delta_total.saturating_sub(delta_idle);
    let system = Some((busy as f32 / delta_total as f32) * 100.0);

    (process, system)
}

#[derive(Debug, Clone)]
struct RawSample {
    process_cpu_ticks: u64,
    total_cpu_ticks: u64,
    idle_cpu_ticks: u64,
    cpu_count: u64,
    process: ProcessRaw,
    system_memory: SystemMemoryRaw,
}

#[derive(Debug, Clone, Default)]
struct ProcessRaw {
    rss_bytes: Option<u64>,
    virtual_bytes: Option<u64>,
    thread_count: Option<u64>,
    fd_count: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct SystemMemoryRaw {
    used_percent: Option<f32>,
    used_bytes: Option<u64>,
    total_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
fn read_raw_sample() -> Result<RawSample, String> {
    let (process_cpu_ticks, process) = read_process_sample()?;
    let (total_cpu_ticks, idle_cpu_ticks, cpu_count) = read_cpu_sample()?;
    let system_memory = read_system_memory();

    Ok(RawSample {
        process_cpu_ticks,
        total_cpu_ticks,
        idle_cpu_ticks,
        cpu_count,
        process,
        system_memory,
    })
}

#[cfg(not(target_os = "linux"))]
fn read_raw_sample() -> Result<RawSample, String> {
    Err("performance sampling currently requires Linux /proc".to_string())
}

#[cfg(target_os = "linux")]
fn read_process_sample() -> Result<(u64, ProcessRaw), String> {
    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|e| format!("failed to read /proc/self/stat: {e}"))?;
    let process_cpu_ticks = parse_process_cpu_ticks(&stat)
        .ok_or_else(|| "failed to parse /proc/self/stat CPU fields".to_string())?;

    let mut process = read_process_status();
    process.fd_count = std::fs::read_dir("/proc/self/fd")
        .ok()
        .map(|entries| entries.filter_map(Result::ok).count() as u64);

    Ok((process_cpu_ticks, process))
}

#[cfg(target_os = "linux")]
fn read_cpu_sample() -> Result<(u64, u64, u64), String> {
    let stat = std::fs::read_to_string("/proc/stat")
        .map_err(|e| format!("failed to read /proc/stat: {e}"))?;
    parse_cpu_line(&stat).ok_or_else(|| "failed to parse /proc/stat CPU line".to_string())
}

#[cfg(target_os = "linux")]
fn read_process_status() -> ProcessRaw {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return ProcessRaw::default();
    };

    let mut process = ProcessRaw::default();
    for line in status.lines() {
        if let Some(bytes) = parse_status_kb(line, "VmRSS:") {
            process.rss_bytes = Some(bytes);
        } else if let Some(bytes) = parse_status_kb(line, "VmSize:") {
            process.virtual_bytes = Some(bytes);
        } else if let Some(threads) = parse_status_number(line, "Threads:") {
            process.thread_count = Some(threads);
        }
    }
    process
}

#[cfg(target_os = "linux")]
fn read_system_memory() -> SystemMemoryRaw {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return SystemMemoryRaw::default();
    };

    let mut total_bytes = None;
    let mut available_bytes = None;
    for line in meminfo.lines() {
        if let Some(bytes) = parse_status_kb(line, "MemTotal:") {
            total_bytes = Some(bytes);
        } else if let Some(bytes) = parse_status_kb(line, "MemAvailable:") {
            available_bytes = Some(bytes);
        }
    }

    let used_bytes = match (total_bytes, available_bytes) {
        (Some(total), Some(available)) => Some(total.saturating_sub(available)),
        _ => None,
    };
    let used_percent = match (used_bytes, total_bytes) {
        (Some(used), Some(total)) if total > 0 => Some((used as f32 / total as f32) * 100.0),
        _ => None,
    };

    SystemMemoryRaw {
        used_percent,
        used_bytes,
        total_bytes,
    }
}

#[cfg(target_os = "linux")]
fn parse_process_cpu_ticks(stat: &str) -> Option<u64> {
    let after_comm = stat.rsplit_once(") ")?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    Some(utime.saturating_add(stime))
}

#[cfg(target_os = "linux")]
fn parse_cpu_line(stat: &str) -> Option<(u64, u64, u64)> {
    let mut cpu_count = 0;
    let mut aggregate = None;

    for line in stat.lines() {
        if let Some(rest) = line.strip_prefix("cpu ") {
            let ticks: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|field| field.parse::<u64>().ok())
                .collect();
            let total = ticks.iter().copied().sum::<u64>();
            let idle = ticks.get(3).copied().unwrap_or(0) + ticks.get(4).copied().unwrap_or(0);
            aggregate = Some((total, idle));
        } else if line
            .strip_prefix("cpu")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
        {
            cpu_count += 1;
        }
    }

    let (total, idle) = aggregate?;
    Some((total, idle, cpu_count.max(1)))
}

#[cfg(target_os = "linux")]
fn parse_status_kb(line: &str, key: &str) -> Option<u64> {
    parse_status_number(line, key).map(|kb| kb.saturating_mul(1024))
}

#[cfg(target_os = "linux")]
fn parse_status_number(line: &str, key: &str) -> Option<u64> {
    let rest = line.strip_prefix(key)?;
    rest.split_whitespace().next()?.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_process_cpu_ticks_after_comm() {
        let stat = "123 (clogger gui) S 1 2 3 4 5 6 7 8 9 10 111 222 13 14 15 16 17 18 19 20";
        assert_eq!(parse_process_cpu_ticks(stat), Some(333));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_cpu_aggregate_and_count() {
        let stat = "cpu  100 10 50 800 40 0 0 0 0 0\ncpu0 50 5 25 400 20 0 0 0 0 0\ncpu1 50 5 25 400 20 0 0 0 0 0\n";
        assert_eq!(parse_cpu_line(stat), Some((1000, 840, 2)));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_status_values() {
        assert_eq!(
            parse_status_kb("VmRSS:\t  1234 kB", "VmRSS:"),
            Some(1_263_616)
        );
        assert_eq!(parse_status_number("Threads:\t9", "Threads:"), Some(9));
    }
}
