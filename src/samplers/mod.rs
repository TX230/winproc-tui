mod counters;
mod cpu;
mod disk;
pub(crate) mod gpu;
pub(crate) mod memory;
pub(crate) mod open_files;
pub(crate) mod pdh;
pub(crate) mod process;
pub(crate) mod process_info;

use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use chrono::Local;
use sysinfo::{ProcessesToUpdate, System};

use crate::model::{GpuCapacitySample, GpuUsageSample, ProcessExtraMetrics, ProcessRow, Snapshot};

pub(crate) use counters::{ProcessCounterSampler, SystemCounterSampler};
use cpu::collect_cpu_summary;
use disk::collect_disk_usages;
use gpu::{GpuSampler, collect_gpu_capacity, collect_gpu_summary_usage, collect_process_gpu_usage};
use memory::map_memory_counters;
use process::collect_process_extras;

pub(crate) struct CollectSnapshotResult {
    pub(crate) snapshot: Snapshot,
    pub(crate) warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleRequest {
    Sample,
    Stop,
}

pub(crate) struct SamplingWorker {
    pub(crate) request_tx: Sender<SampleRequest>,
    pub(crate) result_rx: Receiver<CollectSnapshotResult>,
    pub(crate) join_handle: Option<JoinHandle<()>>,
}

pub(crate) struct SamplingRuntime {
    system: System,
    system_sampler: Option<SystemCounterSampler>,
    process_sampler: Option<ProcessCounterSampler>,
    gpu_sampler: Option<GpuSampler>,
    options: SamplingOptions,
    sample_index: u64,
    cached_slow_process_extras: HashMap<u32, ProcessExtraMetrics>,
    cached_gpu_summary_usage: GpuUsageSample,
    cached_gpu_capacity: GpuCapacitySample,
}

const SLOW_SAMPLE_INTERVAL: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SamplingOptions {
    pub(crate) collect_ws_share: bool,
    pub(crate) collect_gpu: bool,
    pub(crate) collect_gui_resources: bool,
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self {
            collect_ws_share: false,
            collect_gpu: true,
            collect_gui_resources: true,
        }
    }
}

impl SamplingRuntime {
    pub(crate) fn new(options: SamplingOptions) -> Self {
        Self {
            system: System::new_all(),
            system_sampler: SystemCounterSampler::new().ok(),
            process_sampler: ProcessCounterSampler::new().ok(),
            gpu_sampler: options
                .collect_gpu
                .then(|| GpuSampler::new().ok())
                .flatten(),
            options,
            sample_index: 0,
            cached_slow_process_extras: HashMap::new(),
            cached_gpu_summary_usage: GpuUsageSample::default(),
            cached_gpu_capacity: GpuCapacitySample::default(),
        }
    }

    pub(crate) fn collect(&mut self) -> CollectSnapshotResult {
        let collect_slow_metrics = self.sample_index % SLOW_SAMPLE_INTERVAL == 0;
        self.sample_index = self.sample_index.saturating_add(1);
        collect_snapshot(
            &mut self.system,
            self.system_sampler.as_mut(),
            self.process_sampler.as_mut(),
            self.gpu_sampler.as_mut(),
            collect_slow_metrics,
            self.options,
            &mut self.cached_slow_process_extras,
            &mut self.cached_gpu_summary_usage,
            &mut self.cached_gpu_capacity,
        )
    }
}

impl SamplingWorker {
    pub(crate) fn spawn(options: SamplingOptions) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<SampleRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CollectSnapshotResult>();
        let join_handle = thread::spawn(move || {
            let mut runtime = SamplingRuntime::new(options);
            while let Ok(request) = request_rx.recv() {
                match request {
                    SampleRequest::Sample => {
                        let result = runtime.collect();
                        if result_tx.send(result).is_err() {
                            break;
                        }
                    }
                    SampleRequest::Stop => break,
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }

    pub(crate) fn request_sample(&self) -> Result<()> {
        self.request_tx
            .send(SampleRequest::Sample)
            .context("sampling worker is unavailable")
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<CollectSnapshotResult, TryRecvError> {
        self.result_rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, Receiver<SampleRequest>, Sender<CollectSnapshotResult>) {
        let (request_tx, request_rx) = mpsc::channel::<SampleRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CollectSnapshotResult>();
        (
            Self {
                request_tx,
                result_rx,
                join_handle: None,
            },
            request_rx,
            result_tx,
        )
    }
}

impl Drop for SamplingWorker {
    fn drop(&mut self) {
        let _ = self.request_tx.send(SampleRequest::Stop);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

fn collect_snapshot(
    system: &mut System,
    mut system_sampler: Option<&mut SystemCounterSampler>,
    process_sampler: Option<&mut ProcessCounterSampler>,
    gpu_sampler: Option<&mut GpuSampler>,
    collect_slow_metrics: bool,
    options: SamplingOptions,
    cached_slow_process_extras: &mut HashMap<u32, ProcessExtraMetrics>,
    cached_gpu_summary_usage: &mut GpuUsageSample,
    cached_gpu_capacity: &mut GpuCapacitySample,
) -> CollectSnapshotResult {
    system.refresh_memory();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.refresh_cpu_all();

    let logical_processor_count = system.cpus().len().max(1);
    let process_pdh_metrics = process_sampler
        .map(|sampler| sampler.sample(logical_processor_count))
        .unwrap_or_default();
    let process_extras = collect_process_extras(
        process_pdh_metrics,
        collect_slow_metrics,
        options,
        gpu_sampler,
        cached_slow_process_extras,
    );
    let gpu_summary_usage = if !options.collect_gpu {
        GpuUsageSample::default()
    } else if collect_slow_metrics {
        let usage = collect_gpu_summary_usage().merge(collect_process_gpu_usage(&process_extras));
        *cached_gpu_summary_usage = usage;
        usage
    } else {
        *cached_gpu_summary_usage
    };
    let gpu_capacity = if !options.collect_gpu {
        GpuCapacitySample::default()
    } else if collect_slow_metrics {
        let capacity = collect_gpu_capacity();
        *cached_gpu_capacity = capacity.clone();
        capacity
    } else {
        cached_gpu_capacity.clone()
    };
    let disks = collect_disk_usages();

    let mut processes = system
        .processes()
        .values()
        .map(|process| {
            let pid = process.pid().as_u32();
            let extras = process_extras.get(&pid).cloned().unwrap_or_default();
            let workset_bytes = extras.workset_bytes.or(Some(process.memory()));
            ProcessRow {
                pid,
                name: process.name().to_string_lossy().into_owned(),
                start_time: Some(process.start_time()).filter(|value| *value > 0),
                cpu_percent: extras.cpu_percent,
                private_bytes: extras.private_bytes.or(Some(process.virtual_memory())),
                workset_bytes,
                workset_private_bytes: extras.workset_private_bytes,
                workset_shareable_bytes: extras.workset_shareable_bytes,
                workset_shared_bytes: extras.workset_shared_bytes,
                thread_count: extras.thread_count,
                handle_count: extras.handle_count,
                user_object_count: extras.user_object_count,
                gdi_object_count: extras.gdi_object_count,
                gpu_percent: extras.gpu_percent,
                gpu_dedicated_bytes: extras.gpu_dedicated_bytes,
                gpu_shared_bytes: extras.gpu_shared_bytes,
                dotnet_heap_bytes: extras.dotnet_heap_bytes,
                io_read_bytes_per_sec: extras.io_read_bytes_per_sec,
                io_write_bytes_per_sec: extras.io_write_bytes_per_sec,
            }
        })
        .collect::<Vec<_>>();

    processes.sort_by(|left, right| {
        right
            .workset_bytes
            .unwrap_or(0)
            .cmp(&left.workset_bytes.unwrap_or(0))
            .then_with(|| {
                right
                    .private_bytes
                    .unwrap_or(0)
                    .cmp(&left.private_bytes.unwrap_or(0))
            })
            .then_with(|| left.name.cmp(&right.name))
    });

    let total_memory = system.total_memory();
    let fallback_available_memory = system.available_memory();
    let sampled_counters = system_sampler
        .as_mut()
        .map(|sampler| sampler.sample())
        .transpose();
    let cpu_frequencies_mhz = sampled_counters
        .as_ref()
        .ok()
        .and_then(|sample| sample.as_ref())
        .map(|sample| sample.cpu_frequencies_mhz.as_slice())
        .unwrap_or(&[]);
    let cpu_summary = collect_cpu_summary(system, cpu_frequencies_mhz);

    let (
        available_memory,
        committed_memory,
        commit_limit,
        _cache_bytes,
        _standby_cache_bytes,
        _disk_read_bytes_per_sec,
        _disk_write_bytes_per_sec,
        _network_received_bytes_per_sec,
        _network_sent_bytes_per_sec,
        warning,
    ) = map_memory_counters(total_memory, fallback_available_memory, sampled_counters);
    let used_memory = total_memory.saturating_sub(available_memory);

    CollectSnapshotResult {
        snapshot: Snapshot {
            captured_at: Local::now(),
            total_memory,
            used_memory,
            committed_memory,
            commit_limit,
            gpu_dedicated_used: gpu_summary_usage.dedicated,
            gpu_dedicated_total: gpu_capacity.dedicated_total,
            gpu_shared_used: gpu_summary_usage.shared,
            gpu_shared_total: gpu_capacity.shared_total,
            cpu_name: cpu_summary.name,
            cpu_frequency_mhz: cpu_summary.frequency_mhz,
            cpu_current_frequency_mhz: cpu_summary.current_frequency_mhz,
            cpu_p_core_frequency_mhz: cpu_summary.p_core_frequency_mhz,
            cpu_e_core_frequency_mhz: cpu_summary.e_core_frequency_mhz,
            cpu_total_usage_percent: cpu_summary.total_usage_percent,
            cpu_logical_processors: cpu_summary.logical_processors,
            cpu_topology: cpu_summary.topology,
            cpu_cache: cpu_summary.caches,
            gpu_name: gpu_capacity.name,
            disks,
            process_count: processes.len(),
            processes,
        },
        warning,
    }
}
