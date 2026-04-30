//! Lightweight process/system performance sampling for the GUI.
//!
//! `sysinfo` keeps previous measurements internally for CPU deltas, so the
//! sampler owns one long-lived `System` instance and refreshes it once per UI
//! tick instead of recreating it per render.

use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

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
    pid: Result<sysinfo::Pid, String>,
    system: System,
    snapshot: PerfSnapshot,
}

impl PerfSampler {
    pub fn new() -> Self {
        let mut sampler = Self {
            started: Instant::now(),
            pid: sysinfo::get_current_pid().map_err(|e| format!("failed to read current pid: {e}")),
            system: System::new_all(),
            snapshot: PerfSnapshot::default(),
        };
        sampler.sample();
        sampler
    }

    pub fn sample(&mut self) {
        let uptime = self.started.elapsed();
        let sample_count = self.snapshot.sample_count.saturating_add(1);

        if !sysinfo::IS_SUPPORTED_SYSTEM {
            self.snapshot.uptime = uptime;
            self.snapshot.sample_count = sample_count;
            self.snapshot.error = Some("performance sampling is not supported on this OS".into());
            return;
        }

        let pid = match self.pid {
            Ok(pid) => pid,
            Err(ref error) => {
                self.snapshot.uptime = uptime;
                self.snapshot.sample_count = sample_count;
                self.snapshot.error = Some(error.clone());
                return;
            }
        };

        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            process_refresh_kind(),
        );

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let process = self.system.process(pid);

        self.snapshot = PerfSnapshot {
            process_cpu_percent: process.map(|p| p.cpu_usage()),
            system_cpu_percent: Some(self.system.global_cpu_usage()),
            rss_bytes: process.map(|p| p.memory()),
            virtual_bytes: process.map(|p| p.virtual_memory()),
            system_memory_used_percent: percent(used_memory, total_memory),
            system_memory_used_bytes: Some(used_memory),
            system_memory_total_bytes: Some(total_memory),
            thread_count: process.and_then(|p| p.tasks().map(|tasks| tasks.len() as u64)),
            fd_count: process.and_then(|p| p.open_files().map(|n| n as u64)),
            uptime,
            sample_count,
            error: process
                .is_none()
                .then(|| format!("current process {pid} was not found")),
        };
    }

    pub fn snapshot(&self) -> &PerfSnapshot {
        &self.snapshot
    }
}

fn process_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_tasks()
}

fn percent(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| (used as f32 / total as f32) * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_handles_empty_total() {
        assert_eq!(percent(1, 0), None);
        assert_eq!(percent(25, 100), Some(25.0));
    }
}
