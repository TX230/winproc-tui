mod counters;
mod cpu;
mod disk;
mod dotnet_runtime;
pub(crate) mod gpu;
pub(crate) mod memory;
pub(crate) mod open_files;
pub(crate) mod pdh;
pub(crate) mod process;
pub(crate) mod process_environment;
pub(crate) mod process_info;
pub(crate) mod process_modules;

use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use chrono::Local;
use sysinfo::{ProcessesToUpdate, System};

use crate::model::{GpuSample, ProcessExtraMetrics, ProcessRow, Snapshot};

pub(crate) use counters::{ProcessCounterSampler, SystemCounterSampler};
use cpu::collect_cpu_summary;
use disk::collect_disk_usages;
use dotnet_runtime::DotNetRuntimeSampler;
use gpu::{GpuSampler, merge_process_gpu_metrics};
use memory::{collect_performance_info, map_memory_counters};
use process::collect_process_extras;

pub(crate) struct CollectSnapshotResult {
    pub(crate) snapshot: Snapshot,
    pub(crate) warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SampleRequest {
    Sample,
    SuspendDotNet,
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
    dotnet_runtime_sampler: DotNetRuntimeSampler,
}

const SLOW_SAMPLE_INTERVAL: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SamplingOptions {
    pub(crate) collect_gpu: bool,
    pub(crate) collect_gui_resources: bool,
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self {
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
            dotnet_runtime_sampler: DotNetRuntimeSampler::new(),
        }
    }

    pub(crate) fn collect(&mut self) -> CollectSnapshotResult {
        self.collect_internal(false)
    }

    fn collect_live(&mut self) -> CollectSnapshotResult {
        self.collect_internal(true)
    }

    fn collect_internal(&mut self, collect_dotnet: bool) -> CollectSnapshotResult {
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
            &mut self.dotnet_runtime_sampler,
            collect_dotnet,
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
                        let result = runtime.collect_live();
                        if result_tx.send(result).is_err() {
                            break;
                        }
                    }
                    SampleRequest::SuspendDotNet => runtime.dotnet_runtime_sampler.suspend(),
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

    pub(crate) fn suspend_dotnet(&self) {
        let _ = self.request_tx.send(SampleRequest::SuspendDotNet);
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
    mut gpu_sampler: Option<&mut GpuSampler>,
    collect_slow_metrics: bool,
    options: SamplingOptions,
    cached_slow_process_extras: &mut HashMap<u32, ProcessExtraMetrics>,
    dotnet_runtime_sampler: &mut DotNetRuntimeSampler,
    collect_dotnet: bool,
) -> CollectSnapshotResult {
    system.refresh_memory();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.refresh_cpu_all();

    let logical_processor_count = system.cpus().len().max(1);
    let mut process_pdh_metrics = process_sampler
        .map(|sampler| sampler.sample(logical_processor_count))
        .unwrap_or_default();
    let gpu_sample = if options.collect_gpu {
        gpu_sampler
            .as_mut()
            .and_then(|sampler| sampler.sample())
            .unwrap_or_default()
    } else {
        GpuSample::default()
    };
    merge_process_gpu_metrics(&mut process_pdh_metrics, &gpu_sample);
    let process_extras = collect_process_extras(
        process_pdh_metrics,
        collect_slow_metrics,
        options,
        cached_slow_process_extras,
    );
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
                executable_path: process
                    .exe()
                    .map(|path| path.display().to_string())
                    .filter(|path| !path.is_empty()),
                start_time: Some(process.start_time()).filter(|value| *value > 0),
                cpu_percent: extras.cpu_percent,
                private_bytes: extras.private_bytes.or(Some(process.virtual_memory())),
                workset_bytes,
                workset_private_bytes: extras.workset_private_bytes,
                workset_shareable_bytes: extras.workset_shareable_bytes,
                thread_count: extras.thread_count,
                handle_count: extras.handle_count,
                user_object_count: extras.user_object_count,
                gdi_object_count: extras.gdi_object_count,
                gpu_percent: extras.gpu_percent,
                gpu_dedicated_bytes: extras.gpu_dedicated_bytes,
                gpu_shared_bytes: extras.gpu_shared_bytes,
                dotnet_heap_bytes: extras.dotnet_heap_bytes,
                dotnet_gc_gen0_heap_bytes: None,
                dotnet_gc_gen1_heap_bytes: extras.dotnet_gc_gen1_heap_bytes,
                dotnet_gc_gen2_heap_bytes: extras.dotnet_gc_gen2_heap_bytes,
                dotnet_gc_loh_bytes: extras.dotnet_gc_loh_bytes,
                dotnet_gc_poh_bytes: None,
                dotnet_gc_committed_bytes: None,
                dotnet_gc_fragmentation_bytes: None,
                dotnet_allocation_bytes_per_sec: None,
                io_read_bytes_per_sec: extras.io_read_bytes_per_sec,
                io_write_bytes_per_sec: extras.io_write_bytes_per_sec,
            }
        })
        .collect::<Vec<_>>();

    if collect_dotnet {
        dotnet_runtime_sampler.reconcile_and_apply(&mut processes);
    }

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

    let performance = collect_performance_info();
    let total_memory = performance
        .physical_total_bytes
        .unwrap_or_else(|| system.total_memory());
    let fallback_available_memory = system.available_memory();
    let sampled_counters = system_sampler
        .as_mut()
        .map(|sampler| sampler.sample())
        .transpose();
    let sampled_system_counters = sampled_counters
        .as_ref()
        .ok()
        .and_then(|sample| sample.as_ref());
    let cpu_frequencies_mhz = sampled_system_counters
        .map(|sample| sample.cpu_frequencies_mhz.as_slice())
        .unwrap_or(&[]);
    let cpu_summary = collect_cpu_summary(
        system,
        cpu_frequencies_mhz,
        sampled_system_counters.and_then(|sample| sample.cpu_total_usage_percent),
        sampled_system_counters.and_then(|sample| sample.cpu_user_usage_percent),
        sampled_system_counters.and_then(|sample| sample.cpu_kernel_usage_percent),
    );

    let mapped_counters =
        map_memory_counters(total_memory, fallback_available_memory, sampled_counters);
    let used_memory = total_memory.saturating_sub(
        performance
            .physical_available_bytes
            .unwrap_or(mapped_counters.available_memory),
    );

    CollectSnapshotResult {
        snapshot: Snapshot {
            captured_at: Local::now(),
            total_memory,
            used_memory,
            available_memory: Some(mapped_counters.available_memory),
            modified_memory: mapped_counters.modified_page_list_bytes,
            standby_memory: mapped_counters.standby_cache_bytes,
            free_zeroed_memory: mapped_counters.free_zeroed_bytes,
            committed_memory: mapped_counters.committed_memory,
            commit_limit: mapped_counters.commit_limit,
            paged_pool_memory: performance.paged_pool_bytes,
            nonpaged_pool_memory: performance.nonpaged_pool_bytes,
            pages_input_per_sec: mapped_counters.pages_input_per_sec,
            pages_output_per_sec: mapped_counters.pages_output_per_sec,
            cpu_name: cpu_summary.name,
            cpu_frequency_mhz: cpu_summary.frequency_mhz,
            cpu_current_frequency_mhz: cpu_summary.current_frequency_mhz,
            cpu_p_core_frequency_mhz: cpu_summary.p_core_frequency_mhz,
            cpu_e_core_frequency_mhz: cpu_summary.e_core_frequency_mhz,
            cpu_total_usage_percent: cpu_summary.total_usage_percent,
            cpu_user_usage_percent: cpu_summary.user_usage_percent,
            cpu_kernel_usage_percent: cpu_summary.kernel_usage_percent,
            cpu_logical_processors: cpu_summary.logical_processors,
            cpu_topology: cpu_summary.topology,
            cpu_cache: cpu_summary.caches,
            gpu_adapters: gpu_sample.adapters,
            disks,
            disk_read_bytes_per_sec: mapped_counters.disk_read_bytes_per_sec,
            disk_write_bytes_per_sec: mapped_counters.disk_write_bytes_per_sec,
            disk_queue_length: mapped_counters.disk_queue_length,
            network_received_bytes_per_sec: mapped_counters.network_received_bytes_per_sec,
            network_sent_bytes_per_sec: mapped_counters.network_sent_bytes_per_sec,
            process_count: performance
                .process_count
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(processes.len()),
            thread_count: performance.thread_count,
            processes,
        },
        warning: mapped_counters.warning,
    }
}
