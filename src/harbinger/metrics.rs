use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind,
    ProcessesToUpdate, RefreshKind, System,
};
use tracing::warn;

const MAX_SAMPLES: usize = 120;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn safe_rate(bytes: u64, previous: Option<u64>, elapsed_seconds: f64) -> f64 {
    match previous {
        Some(previous) if bytes >= previous && elapsed_seconds > 0.0 => {
            let rate = (bytes - previous) as f64 / elapsed_seconds;
            if rate.is_finite() && rate >= 0.0 {
                rate.min(1.0e15)
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct MetricSample {
    pub(crate) at: i64,
    pub(crate) host_cpu_percent: f32,
    pub(crate) process_cpu_percent: f32,
    pub(crate) host_memory_used_bytes: u64,
    pub(crate) host_memory_total_bytes: u64,
    pub(crate) host_memory_percent: f32,
    pub(crate) process_memory_bytes: u64,
    pub(crate) network_received_bytes_per_sec: f64,
    pub(crate) network_transmitted_bytes_per_sec: f64,
    pub(crate) disk_read_bytes_per_sec: f64,
    pub(crate) disk_write_bytes_per_sec: f64,
    pub(crate) process_disk_read_bytes_per_sec: f64,
    pub(crate) process_disk_write_bytes_per_sec: f64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiskInfo {
    pub(crate) name: String,
    pub(crate) mount_point: String,
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
    pub(crate) used_percent: f32,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ActiveActivity {
    pub(crate) key: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) started_at: i64,
    pub(crate) elapsed_seconds: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MetricsSnapshot {
    pub(crate) latest: Option<MetricSample>,
    pub(crate) series: Vec<MetricSample>,
    pub(crate) activities: Vec<ActiveActivity>,
    pub(crate) disks: Vec<DiskInfo>,
}

#[derive(Default)]
struct MetricsInner {
    samples: VecDeque<MetricSample>,
    activities: BTreeMap<String, (String, String, String, i64)>,
    disks: Vec<DiskInfo>,
}

#[derive(Clone)]
pub(crate) struct Metrics {
    inner: Arc<Mutex<MetricsInner>>,
}

impl Metrics {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MetricsInner::default())),
        }
    }

    pub(crate) fn record(&self, sample: MetricSample) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.samples.push_back(sample);
            while inner.samples.len() > MAX_SAMPLES {
                inner.samples.pop_front();
            }
        }
    }

    pub(crate) fn begin_activity(
        &self,
        key: impl Into<String>,
        kind: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> bool {
        let key = key.into();
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.activities.contains_key(&key) {
            return false;
        }
        inner.activities.insert(
            key,
            (
                kind.into(),
                title.into(),
                detail.into(),
                unix_seconds(),
            ),
        );
        true
    }

    pub(crate) fn end_activity(&self, key: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.activities.remove(key);
        }
    }

    pub(crate) fn snapshot(&self) -> MetricsSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return MetricsSnapshot::default();
        };
        let now = unix_seconds();
        let activities = inner
            .activities
            .iter()
            .map(|(key, (kind, title, detail, started_at))| ActiveActivity {
                key: key.clone(),
                kind: kind.clone(),
                title: title.clone(),
                detail: detail.clone(),
                started_at: *started_at,
                elapsed_seconds: now.saturating_sub(*started_at) as u64,
            })
            .collect();
        MetricsSnapshot {
            latest: inner.samples.back().cloned(),
            series: inner.samples.iter().cloned().collect(),
            activities,
            disks: inner.disks.clone(),
        }
    }

    fn set_disks(&self, disks: Vec<DiskInfo>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.disks = disks;
        }
    }
}

pub(crate) struct ActivityGuard {
    metrics: Metrics,
    key: Option<String>,
}

impl ActivityGuard {
    pub(crate) fn begin(
        metrics: &Metrics,
        key: impl Into<String>,
        kind: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let key = key.into();
        let started = metrics.begin_activity(key.clone(), kind, title, detail);
        Self {
            metrics: metrics.clone(),
            key: started.then_some(key),
        }
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.metrics.end_activity(&key);
        }
    }
}

fn collect_disks() -> (Vec<DiskInfo>, (u64, u64)) {
    let Ok(mut disks) =
        Disks::new_with_refreshed_list_specifics(DiskRefreshKind::everything())
    else {
        return (Vec::new(), (0, 0));
    };
    disks.refresh_specifics(false, DiskRefreshKind::everything());
    let mut info = Vec::new();
    let mut read_bytes = 0_u64;
    let mut written_bytes = 0_u64;
    for disk in &disks {
        let usage = disk.usage();
        read_bytes = read_bytes.saturating_add(usage.total_read_bytes);
        written_bytes = written_bytes.saturating_add(usage.total_written_bytes);
        let total = disk.total_space();
        let available = disk.available_space();
        let used_percent = if total == 0 {
            0.0
        } else {
            ((total.saturating_sub(available)) as f32 / total as f32) * 100.0
        };
        info.push(DiskInfo {
            name: disk.name().to_string_lossy().into_owned(),
            mount_point: disk.mount_point().to_string_lossy().into_owned(),
            total_bytes: total,
            available_bytes: available,
            used_percent,
        });
    }
    info.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
    (info, (read_bytes, written_bytes))
}

fn is_loopback(name: &str) -> bool {
    name.starts_with("lo") || name.contains("Loopback")
}

pub(crate) async fn run_sampler(metrics: Metrics) {
    let process_kind = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_disk_usage();
    let mut system = match System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything())
            .with_processes(process_kind),
    ) {
        Ok(system) => system,
        Err(error) => {
            warn!(%error, "server metrics sampler could not read system information");
            return;
        }
    };
    let mut networks = match Networks::new_with_refreshed_list() {
        Ok(networks) => networks,
        Err(error) => {
            warn!(%error, "server metrics sampler could not read network information");
            return;
        }
    };
    let Some(pid) = sysinfo::get_current_pid().ok() else {
        warn!("server metrics sampler could not identify its own process");
        return;
    };

    let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
    let mut last = Instant::now();
    let mut previous_network: Option<(u64, u64)> = None;
    let mut previous_disk_io: Option<(u64, u64)> = None;
    let mut previous_process_io: Option<(u64, u64)> = None;

    loop {
        ticker.tick().await;
        let now = Instant::now();
        let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
        last = now;

        system.refresh_cpu_usage();
        system.refresh_memory();
        let pids = [pid];
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), false, process_kind);
        networks.refresh(true);
        let (disks, disk_io) = collect_disks();
        metrics.set_disks(disks);

        let network_total = networks.iter().fold((0_u64, 0_u64), |totals, (name, data)| {
            if is_loopback(name.to_string_lossy().as_ref()) {
                totals
            } else {
                (
                    totals.0.saturating_add(data.total_received()),
                    totals.1.saturating_add(data.total_transmitted()),
                )
            }
        });
        let process_io = system
            .process(pid)
            .map(|process| process.disk_usage())
            .map(|usage| (usage.total_read_bytes, usage.total_written_bytes))
            .unwrap_or((0, 0));

        let host_total = system.total_memory();
        let host_used = system.used_memory();
        let sample = MetricSample {
            at: unix_seconds(),
            host_cpu_percent: system.global_cpu_usage(),
            process_cpu_percent: system
                .process(pid)
                .map(|process| process.cpu_usage())
                .unwrap_or_default(),
            host_memory_used_bytes: host_used,
            host_memory_total_bytes: host_total,
            host_memory_percent: if host_total == 0 {
                0.0
            } else {
                (host_used as f32 / host_total as f32) * 100.0
            },
            process_memory_bytes: system
                .process(pid)
                .map(|process| process.memory())
                .unwrap_or_default(),
            network_received_bytes_per_sec: safe_rate(network_total.0, previous_network.map(|v| v.0), elapsed),
            network_transmitted_bytes_per_sec: safe_rate(network_total.1, previous_network.map(|v| v.1), elapsed),
            disk_read_bytes_per_sec: safe_rate(disk_io.0, previous_disk_io.map(|v| v.0), elapsed),
            disk_write_bytes_per_sec: safe_rate(disk_io.1, previous_disk_io.map(|v| v.1), elapsed),
            process_disk_read_bytes_per_sec: safe_rate(process_io.0, previous_process_io.map(|v| v.0), elapsed),
            process_disk_write_bytes_per_sec: safe_rate(process_io.1, previous_process_io.map(|v| v.1), elapsed),
        };
        previous_network = Some(network_total);
        previous_disk_io = Some(disk_io);
        previous_process_io = Some(process_io);
        metrics.record(sample);
    }
}
