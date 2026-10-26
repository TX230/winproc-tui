use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::model::{ProcessIdentity, ProcessRow};

const IPC_MAGIC: &[u8; 14] = b"DOTNET_IPC_V1\0";
const IPC_UNKNOWN_COMMAND: u32 = 0x8013_1385;
const IPC_NOT_SUPPORTED: u32 = 0x8013_1515;
const METRICS_PROVIDER: &str = "System.Diagnostics.Metrics";
const METRICS_METER: &str = "System.Runtime";
const EVENT_COUNTER_PROVIDER: &str = "System.Runtime";
const EVENT_COUNTER_NAME: &str = "EventCounters";
const EVENT_COUNTER_HEAP_SIZE: &str = "gc-heap-size";
const EVENT_COUNTER_COMMITTED: &str = "gc-committed";
const EVENT_COUNTER_FRAGMENTATION: &str = "gc-fragmentation";
const EVENT_COUNTER_ALLOCATION_RATE: &str = "alloc-rate";
const EVENT_COUNTER_GEN0_SIZE: &str = "gen-0-size";
const EVENT_COUNTER_GEN1_SIZE: &str = "gen-1-size";
const EVENT_COUNTER_GEN2_SIZE: &str = "gen-2-size";
const EVENT_COUNTER_LOH_SIZE: &str = "loh-size";
const EVENT_COUNTER_POH_SIZE: &str = "poh-size";
const METRIC_STALE_AFTER: Duration = Duration::from_secs(3);
const FIRST_METRIC_TIMEOUT: Duration = Duration::from_secs(5);

const MAX_BLOCK_SIZE: usize = 16 * 1024 * 1024;
const MAX_TYPE_NAME_SIZE: usize = 64;

const EVENT_COLLECTION_START: u32 = 2;
const EVENT_COLLECTION_STOP: u32 = 3;
const EVENT_COUNTER_RATE: u32 = 4;
const EVENT_UP_DOWN_COUNTER_RATE: u32 = 16;

const GC_TOTAL_ALLOCATED: &str = "dotnet.gc.heap.total_allocated";
const GC_COMMITTED: &str = "dotnet.gc.last_collection.memory.committed_size";
const GC_HEAP_SIZE: &str = "dotnet.gc.last_collection.heap.size";
const GC_FRAGMENTATION: &str = "dotnet.gc.last_collection.heap.fragmentation.size";

const GENERATION_TAGS: [&str; 5] = ["gen0", "gen1", "gen2", "loh", "poh"];

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DotNetRuntimeMetrics {
    pub(crate) heap_bytes: Option<u64>,
    pub(crate) gen0_heap_bytes: Option<u64>,
    pub(crate) gen1_heap_bytes: Option<u64>,
    pub(crate) gen2_heap_bytes: Option<u64>,
    pub(crate) loh_bytes: Option<u64>,
    pub(crate) poh_bytes: Option<u64>,
    pub(crate) committed_bytes: Option<u64>,
    pub(crate) fragmentation_bytes: Option<u64>,
    pub(crate) allocation_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone)]
struct PublishedMetrics {
    collected_at: Instant,
    metrics: DotNetRuntimeMetrics,
}

pub(crate) struct DotNetRuntimeSampler {
    pub(crate) control: super::dotnet_control::DotNetControl,
    sessions: HashMap<ProcessIdentity, RuntimeSession>,
    #[cfg(not(test))]
    detection: Option<super::dotnet_detection::DotNetDetection>,
    detected: HashSet<ProcessIdentity>,
    rejected: HashSet<ProcessIdentity>,
}

impl DotNetRuntimeSampler {
    pub(crate) fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            #[cfg(not(test))]
            detection: None,
            control: Default::default(),
            detected: HashSet::new(),
            rejected: HashSet::new(),
        }
    }

    pub(crate) fn reconcile_and_apply(&mut self, processes: &mut [ProcessRow]) {
        let live = processes
            .iter()
            .map(ProcessIdentity::from_row)
            .collect::<HashSet<_>>();

        self.control.retain(&live);
        #[cfg(not(test))]
        self.detection
            .get_or_insert_with(|| {
                super::dotnet_detection::DotNetDetection::new(self.control.clone())
            })
            .request(live.iter().cloned().collect());
        self.sessions
            .retain(|identity, _| self.control.allows(identity));
        self.detected.retain(|identity| live.contains(identity));
        self.rejected.retain(|identity| live.contains(identity));

        let finished = self
            .sessions
            .iter()
            .filter(|(_, session)| session.is_finished() || session.first_metric_timed_out())
            .map(|(identity, session)| (identity.clone(), session.first_metric_timed_out()))
            .collect::<Vec<_>>();
        for (identity, timed_out) in finished {
            self.sessions.remove(&identity);
            self.rejected.insert(identity.clone());
            if !self.control.status(&identity).starts_with("Unavailable:") {
                self.control.report(
                    &identity,
                    if timed_out {
                        "Unavailable: no .NET metrics received within 5s; EventSource may be disabled; toggle off/on to retry"
                    } else {
                        "Unavailable: session ended; toggle off/on to retry"
                    }.into(),
                );
            }
        }

        for identity in live {
            if !self.should_start(&identity) {
                continue;
            }
            match RuntimeSession::try_spawn(identity.clone(), self.control.clone()) {
                Ok(session) => {
                    self.detected.insert(identity.clone());
                    self.sessions.insert(identity, session);
                }
                Err(error) => {
                    self.control
                        .report(&identity, format!("Unavailable: {error}"));
                    self.rejected.insert(identity);
                }
            }
        }

        for process in processes {
            let identity = ProcessIdentity::from_row(process);
            let Some(metrics) = self
                .sessions
                .get(&identity)
                .and_then(RuntimeSession::latest)
            else {
                if self.sessions.contains_key(&identity)
                    && !self.control.status(&identity).starts_with("Unavailable:")
                {
                    self.control.report(&identity, "Waiting".into());
                }
                continue;
            };

            process.dotnet_eventpipe = Some(self.control.generation());
            self.control.report(&identity, "Collecting".into());
            if let Some(heap_bytes) = metrics.heap_bytes {
                process.dotnet_heap_bytes = Some(heap_bytes);
            }
            if let Some(value) = metrics.gen0_heap_bytes {
                process.dotnet_gc_gen0_heap_bytes = Some(value);
            }
            if let Some(value) = metrics.gen1_heap_bytes {
                process.dotnet_gc_gen1_heap_bytes = Some(value);
            }
            if let Some(value) = metrics.gen2_heap_bytes {
                process.dotnet_gc_gen2_heap_bytes = Some(value);
            }
            if let Some(value) = metrics.loh_bytes {
                process.dotnet_gc_loh_bytes = Some(value);
            }
            if let Some(value) = metrics.poh_bytes {
                process.dotnet_gc_poh_bytes = Some(value);
            }
            process.dotnet_gc_committed_bytes = metrics.committed_bytes;
            process.dotnet_gc_fragmentation_bytes = metrics.fragmentation_bytes;
            process.dotnet_allocation_bytes_per_sec = metrics.allocation_bytes_per_sec;
        }
    }

    pub(crate) fn remove_targets(&mut self, identities: &[ProcessIdentity]) {
        for identity in identities {
            self.sessions.remove(identity);
            self.detected.remove(identity);
            self.rejected.remove(identity);
        }
        self.control.finish_remove(identities);
    }

    pub(crate) fn suspend(&mut self) {
        self.control.disable();
        self.sessions.clear();
        self.detected.clear();
        self.rejected.clear();
        self.control.finish_suspend();
    }

    fn should_start(&self, identity: &ProcessIdentity) -> bool {
        self.control.allows(identity)
            && !self.sessions.contains_key(identity)
            && !self.rejected.contains(identity)
    }
}

impl Drop for DotNetRuntimeSampler {
    fn drop(&mut self) {
        self.suspend();
    }
}

struct RuntimeSession {
    started_at: Instant,
    permit: Arc<super::dotnet_control::Permit>,
    identity: ProcessIdentity,
    latest: Arc<Mutex<Option<PublishedMetrics>>>,
    session_id: Arc<AtomicU64>,
    stop_requested: Arc<AtomicBool>,
    stop_sent: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl RuntimeSession {
    fn try_spawn(
        identity: ProcessIdentity,
        control: super::dotnet_control::DotNetControl,
    ) -> io::Result<Self> {
        let permit = control
            .permit(&identity)
            .ok_or_else(|| io::Error::other("collection disabled"))?;
        verify_identity(&identity)?;
        let stream = open_diagnostic_pipe(identity.pid, 1)?;
        Ok(Self::spawn(identity, stream, permit, control))
    }

    fn spawn(
        identity: ProcessIdentity,
        stream: File,
        permit: super::dotnet_control::Permit,
        control: super::dotnet_control::DotNetControl,
    ) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let session_id = Arc::new(AtomicU64::new(0));
        let stop_requested = Arc::new(AtomicBool::new(false));
        let stop_sent = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let worker_latest = Arc::clone(&latest);
        let worker_session_id = Arc::clone(&session_id);
        let worker_stop_requested = Arc::clone(&stop_requested);
        let worker_stop_sent = Arc::clone(&stop_sent);
        let worker_finished = Arc::clone(&finished);
        let pid = identity.pid;
        let worker_identity = identity.clone();
        let permit = Arc::new(permit);
        let worker_permit = Arc::clone(&permit);
        let join_handle = thread::Builder::new()
            .name(format!("dotnet-runtime-{pid}"))
            .spawn(move || {
                let _permit = worker_permit;
                let result = collect_runtime_metrics(
                    &worker_identity,
                    &_permit,
                    stream,
                    worker_session_id,
                    worker_stop_requested,
                    worker_stop_sent,
                    worker_latest,
                );
                if let Err(error) = result {
                    control.report(&worker_identity, format!("Unavailable: {error}"));
                }
                worker_finished.store(true, Ordering::Release);
            })
            .ok();

        if join_handle.is_none() {
            finished.store(true, Ordering::Release);
        }

        Self {
            started_at: Instant::now(),
            permit,
            identity,
            latest,
            session_id,
            stop_requested,
            stop_sent,
            finished,
            join_handle,
        }
    }

    fn first_metric_timed_out(&self) -> bool {
        self.started_at.elapsed() >= FIRST_METRIC_TIMEOUT
            && self.latest.lock().is_ok_and(|latest| latest.is_none())
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    fn latest(&self) -> Option<DotNetRuntimeMetrics> {
        let latest = self.latest.lock().ok()?.clone()?;
        (latest.collected_at.elapsed() <= METRIC_STALE_AFTER).then_some(latest.metrics)
    }

    fn stop(&mut self) {
        self.stop_requested.store(true, Ordering::Release);
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        let pid = self.identity.pid;
        let identity = self.identity.clone();
        let permit = Arc::clone(&self.permit);
        let session_id = Arc::clone(&self.session_id);
        let stop_sent = Arc::clone(&self.stop_sent);
        let _ = thread::Builder::new()
            .name(format!("dotnet-runtime-cleanup-{pid}"))
            .spawn(move || {
                let _permit = permit;
                request_eventpipe_stop(&identity, session_id.load(Ordering::Acquire), &stop_sent);
                let _ = join_handle.join();
            });
    }
}

impl Drop for RuntimeSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn collect_runtime_metrics(
    identity: &ProcessIdentity,
    permit: &super::dotnet_control::Permit,
    stream: File,
    session_id: Arc<AtomicU64>,
    stop_requested: Arc<AtomicBool>,
    stop_sent: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<PublishedMetrics>>>,
) -> io::Result<()> {
    let (stream, id) = open_eventpipe_session(identity, permit, stream)?;
    session_id.store(id, Ordering::Release);
    if stop_requested.load(Ordering::Acquire) {
        request_eventpipe_stop(identity, id, &stop_sent);
    }

    let mut accumulator = RuntimeMetricAccumulator::default();
    let mut parser = NetTraceParser::new(stream);
    parser.run(|event, payload| {
        if stop_requested.load(Ordering::Acquire) {
            request_eventpipe_stop(identity, id, &stop_sent);
        }
        let metrics = accumulator.handle_event(event, payload);
        if let Some(metrics) = metrics
            && let Ok(mut current) = latest.lock()
        {
            *current = Some(PublishedMetrics {
                collected_at: Instant::now(),
                metrics,
            });
        }
    })
}

fn open_eventpipe_session(
    identity: &ProcessIdentity,
    permit: &super::dotnet_control::Permit,
    stream: File,
) -> io::Result<(File, u64)> {
    let pid = identity.pid;
    match start_eventpipe_session(stream, identity, permit, CollectCommand::Tracing3) {
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            let stream = open_diagnostic_pipe(pid, 5)?;
            start_eventpipe_session(stream, identity, permit, CollectCommand::Tracing2)
        }
        result => result,
    }
}

fn start_eventpipe_session(
    mut stream: File,
    identity: &ProcessIdentity,
    permit: &super::dotnet_control::Permit,
    command: CollectCommand,
) -> io::Result<(File, u64)> {
    verify_identity(identity)?;
    if permit.cancelled() {
        return Err(io::Error::other("collection disabled"));
    }
    stream.write_all(&collect_tracing_request(identity.pid, command))?;
    stream.flush()?;
    let response = read_ipc_response(&mut stream)?;
    if response.len() != 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "EventPipe session response has an invalid size",
        ));
    }
    let id = u64::from_le_bytes(response.try_into().expect("validated length"));
    Ok((stream, id))
}

fn request_eventpipe_stop(identity: &ProcessIdentity, session_id: u64, stop_sent: &AtomicBool) {
    if session_id == 0
        || stop_sent
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    if verify_identity(identity).is_err() {
        return;
    }
    if stop_eventpipe_session(identity.pid, session_id).is_err() {
        stop_sent.store(false, Ordering::Release);
    }
}

fn open_diagnostic_pipe(pid: u32, attempts: usize) -> io::Result<File> {
    let path = format!(r"\\.\pipe\dotnet-diagnostic-{pid}");
    let mut last_error = None;
    for attempt in 0..attempts.max(1) {
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => return Ok(file),
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < attempts {
            thread::sleep(Duration::from_millis(50));
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("diagnostic pipe is unavailable")))
}

#[derive(Clone, Copy)]
enum CollectCommand {
    Tracing2,
    Tracing3,
}

fn collect_tracing_request(target_pid: u32, command: CollectCommand) -> Vec<u8> {
    let metrics = [
        GC_TOTAL_ALLOCATED,
        GC_COMMITTED,
        GC_HEAP_SIZE,
        GC_FRAGMENTATION,
    ]
    .into_iter()
    .map(|instrument| format!("{METRICS_METER}\\{instrument}"))
    .collect::<Vec<_>>()
    .join(";");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let arguments = format!(
        "SessionId=SHARED;ClientId=winproc-tui-{target_pid}-{}-{nonce};RefreshInterval=1;Metrics=\"{metrics}\";MaxTimeSeries=32;MaxHistograms=0",
        std::process::id()
    );

    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.push(0);
    if matches!(command, CollectCommand::Tracing3) {
        payload.push(0);
    }
    payload.extend_from_slice(&1_u32.to_le_bytes());
    match command {
        CollectCommand::Tracing3 => {
            payload.extend_from_slice(&7_u64.to_le_bytes());
            payload.extend_from_slice(&4_u32.to_le_bytes());
            push_ipc_string(&mut payload, METRICS_PROVIDER);
            push_ipc_string(&mut payload, &arguments);
        }
        CollectCommand::Tracing2 => {
            payload.extend_from_slice(&u64::MAX.to_le_bytes());
            payload.extend_from_slice(&4_u32.to_le_bytes());
            push_ipc_string(&mut payload, EVENT_COUNTER_PROVIDER);
            push_ipc_string(&mut payload, "EventCounterIntervalSec=1");
        }
    }
    ipc_packet(
        match command {
            CollectCommand::Tracing2 => 3,
            CollectCommand::Tracing3 => 4,
        },
        &payload,
    )
}

fn stop_eventpipe_session(pid: u32, session_id: u64) -> io::Result<()> {
    let mut stream = open_diagnostic_pipe(pid, 2)?;
    stream.write_all(&ipc_packet(1, &session_id.to_le_bytes()))?;
    stream.flush()
}

fn ipc_packet(command_id: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(20 + payload.len());
    packet.extend_from_slice(IPC_MAGIC);
    packet.extend_from_slice(
        &u16::try_from(20 + payload.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    packet.push(2);
    packet.push(command_id);
    packet.extend_from_slice(&0_u16.to_le_bytes());
    packet.extend_from_slice(payload);
    packet
}

fn push_ipc_string(buffer: &mut Vec<u8>, value: &str) {
    let utf16 = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    buffer.extend_from_slice(&u32::try_from(utf16.len()).unwrap_or(0).to_le_bytes());
    for unit in utf16 {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
}

fn read_ipc_response(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0_u8; 20];
    stream.read_exact(&mut header)?;
    if &header[..14] != IPC_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "diagnostic IPC response has an invalid magic value",
        ));
    }
    let size = usize::from(u16::from_le_bytes([header[14], header[15]]));
    if size < 20 {
        return Err(invalid_data("diagnostic IPC response has an invalid size"));
    }
    let mut payload = vec![0; size - 20];
    stream.read_exact(&mut payload)?;
    if header[16] != 0xff || header[17] != 0 {
        let error = payload
            .get(..4)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes);
        let kind = match error {
            Some(IPC_UNKNOWN_COMMAND) | Some(IPC_NOT_SUPPORTED) => io::ErrorKind::Unsupported,
            _ => io::ErrorKind::Other,
        };
        return Err(io::Error::new(kind, "diagnostic IPC command was rejected"));
    }
    Ok(payload)
}

#[derive(Default)]
struct MetricAccumulator {
    collecting: bool,
    heap: GenerationValues,
    fragmentation: GenerationValues,
    committed_bytes: Option<u64>,
    allocation_bytes_per_sec: Option<u64>,
}

#[derive(Default)]
struct RuntimeMetricAccumulator {
    modern: MetricAccumulator,
    legacy: EventCounterAccumulator,
}

impl RuntimeMetricAccumulator {
    fn handle_event(&mut self, event: ParsedEvent, payload: &[u8]) -> Option<DotNetRuntimeMetrics> {
        match event {
            ParsedEvent::Metrics(event_id) => self.modern.handle_event(event_id, payload),
            ParsedEvent::RuntimeCounters => self.legacy.handle_event(payload),
        }
    }
}

#[derive(Default)]
struct EventCounterAccumulator {
    seen: u16,
    heap_bytes: Option<u64>,
    gen0_heap_bytes: Option<u64>,
    gen1_heap_bytes: Option<u64>,
    gen2_heap_bytes: Option<u64>,
    loh_bytes: Option<u64>,
    poh_bytes: Option<u64>,
    committed_bytes: Option<u64>,
    fragmentation_percent: Option<f64>,
    allocation_bytes_per_sec: Option<u64>,
}

impl EventCounterAccumulator {
    const ALL: u16 = 0x01ff;

    fn handle_event(&mut self, payload: &[u8]) -> Option<DotNetRuntimeMetrics> {
        let event = EventCounterValue::parse(payload)?;
        let (bit, value) = match event.name.as_str() {
            EVENT_COUNTER_HEAP_SIZE => (1, event.value * 1_000_000.0),
            EVENT_COUNTER_COMMITTED => (2, event.value * 1_000_000.0),
            EVENT_COUNTER_FRAGMENTATION => (4, event.value),
            EVENT_COUNTER_ALLOCATION_RATE => (8, event.value / event.interval_sec),
            EVENT_COUNTER_GEN0_SIZE => (16, event.value),
            EVENT_COUNTER_GEN1_SIZE => (32, event.value),
            EVENT_COUNTER_GEN2_SIZE => (64, event.value),
            EVENT_COUNTER_LOH_SIZE => (128, event.value),
            EVENT_COUNTER_POH_SIZE => (256, event.value),
            _ => return None,
        };
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        if self.seen & bit != 0 {
            *self = Self::default();
        }
        match event.name.as_str() {
            EVENT_COUNTER_HEAP_SIZE => self.heap_bytes = nonnegative_rounded_u64(value),
            EVENT_COUNTER_COMMITTED => {
                self.committed_bytes = nonnegative_rounded_u64(value);
            }
            EVENT_COUNTER_FRAGMENTATION => self.fragmentation_percent = Some(value),
            EVENT_COUNTER_ALLOCATION_RATE => {
                self.allocation_bytes_per_sec = nonnegative_rounded_u64(value);
            }
            EVENT_COUNTER_GEN0_SIZE => self.gen0_heap_bytes = nonnegative_rounded_u64(value),
            EVENT_COUNTER_GEN1_SIZE => self.gen1_heap_bytes = nonnegative_rounded_u64(value),
            EVENT_COUNTER_GEN2_SIZE => self.gen2_heap_bytes = nonnegative_rounded_u64(value),
            EVENT_COUNTER_LOH_SIZE => self.loh_bytes = nonnegative_rounded_u64(value),
            EVENT_COUNTER_POH_SIZE => self.poh_bytes = nonnegative_rounded_u64(value),
            _ => unreachable!(),
        }
        self.seen |= bit;
        if self.seen != Self::ALL {
            return None;
        }

        let fragmentation_bytes =
            nonnegative_rounded_u64(self.heap_bytes? as f64 * self.fragmentation_percent? / 100.0);
        let metrics = DotNetRuntimeMetrics {
            heap_bytes: self.heap_bytes,
            gen0_heap_bytes: self.gen0_heap_bytes,
            gen1_heap_bytes: self.gen1_heap_bytes,
            gen2_heap_bytes: self.gen2_heap_bytes,
            loh_bytes: self.loh_bytes,
            poh_bytes: self.poh_bytes,
            committed_bytes: self.committed_bytes,
            fragmentation_bytes,
            allocation_bytes_per_sec: self.allocation_bytes_per_sec,
        };
        *self = Self::default();
        Some(metrics)
    }
}

struct EventCounterValue {
    name: String,
    value: f64,
    interval_sec: f64,
}

impl EventCounterValue {
    fn parse(payload: &[u8]) -> Option<Self> {
        let mut reader = SliceReader::new(payload);
        let name = reader.utf16z().ok()?;
        let _display_name = reader.utf16z().ok()?;
        let (value, interval_sec) = match name.as_str() {
            EVENT_COUNTER_HEAP_SIZE
            | EVENT_COUNTER_COMMITTED
            | EVENT_COUNTER_FRAGMENTATION
            | EVENT_COUNTER_GEN0_SIZE
            | EVENT_COUNTER_GEN1_SIZE
            | EVENT_COUNTER_GEN2_SIZE
            | EVENT_COUNTER_LOH_SIZE
            | EVENT_COUNTER_POH_SIZE => {
                let value = reader.f64().ok()?;
                let _standard_deviation = reader.f64().ok()?;
                let _count = reader.u32().ok()?;
                let _minimum = reader.f64().ok()?;
                let _maximum = reader.f64().ok()?;
                (value, f64::from(reader.f32().ok()?))
            }
            EVENT_COUNTER_ALLOCATION_RATE => {
                let _display_rate_time_scale = reader.utf16z().ok()?;
                (reader.f64().ok()?, f64::from(reader.f32().ok()?))
            }
            _ => return None,
        };
        (value.is_finite() && value >= 0.0 && interval_sec.is_finite() && interval_sec > 0.0)
            .then_some(Self {
                name,
                value,
                interval_sec,
            })
    }
}

impl MetricAccumulator {
    fn reset(&mut self) {
        *self = Self {
            collecting: true,
            ..Self::default()
        };
    }

    fn handle_event(&mut self, event_id: u32, payload: &[u8]) -> Option<DotNetRuntimeMetrics> {
        match event_id {
            EVENT_COLLECTION_START => self.reset(),
            EVENT_COLLECTION_STOP if self.collecting => {
                self.collecting = false;
                return Some(DotNetRuntimeMetrics {
                    heap_bytes: self.heap.total(),
                    gen0_heap_bytes: self.heap.values[0],
                    gen1_heap_bytes: self.heap.values[1],
                    gen2_heap_bytes: self.heap.values[2],
                    loh_bytes: self.heap.values[3],
                    poh_bytes: self.heap.values[4],
                    committed_bytes: self.committed_bytes,
                    fragmentation_bytes: self.fragmentation.total(),
                    allocation_bytes_per_sec: self.allocation_bytes_per_sec,
                });
            }
            EVENT_COUNTER_RATE if self.collecting => self.handle_counter_rate(payload),
            EVENT_UP_DOWN_COUNTER_RATE if self.collecting => self.handle_up_down_counter(payload),
            _ => {}
        }
        None
    }

    fn handle_counter_rate(&mut self, payload: &[u8]) {
        let Some(event) = MetricValueEvent::parse(payload, true) else {
            return;
        };
        if event.meter != METRICS_METER {
            return;
        }
        let Some(rate) = event
            .rate
            .and_then(parse_finite_f64)
            .filter(|value| *value >= 0.0)
        else {
            return;
        };
        if event.instrument.as_str() == GC_TOTAL_ALLOCATED {
            self.allocation_bytes_per_sec = nonnegative_rounded_u64(rate);
        }
    }

    fn handle_up_down_counter(&mut self, payload: &[u8]) {
        let Some(event) = MetricValueEvent::parse(payload, true) else {
            return;
        };
        if event.meter != METRICS_METER {
            return;
        }
        let Some(value) = event.value.and_then(parse_nonnegative_u64) else {
            return;
        };
        match event.instrument.as_str() {
            GC_COMMITTED if event.tags.is_empty() => self.committed_bytes = Some(value),
            GC_HEAP_SIZE => {
                if let Some(generation) = generation_from_tags(&event.tags) {
                    self.heap.values[generation] = Some(value);
                }
            }
            GC_FRAGMENTATION => {
                if let Some(generation) = generation_from_tags(&event.tags) {
                    self.fragmentation.values[generation] = Some(value);
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct GenerationValues {
    values: [Option<u64>; 5],
}

impl GenerationValues {
    fn total(&self) -> Option<u64> {
        self.values
            .iter()
            .copied()
            .try_fold(0_u64, |total, value| total.checked_add(value?))
    }
}

struct MetricValueEvent {
    meter: String,
    instrument: String,
    tags: String,
    rate: Option<String>,
    value: Option<String>,
}

impl MetricValueEvent {
    fn parse(payload: &[u8], has_rate_and_value: bool) -> Option<Self> {
        let mut reader = SliceReader::new(payload);
        let _session_id = reader.utf16z().ok()?;
        let meter = reader.utf16z().ok()?;
        let _meter_version = reader.utf16z().ok()?;
        let instrument = reader.utf16z().ok()?;
        let _unit = reader.utf16z().ok()?;
        let tags = reader.utf16z().ok()?;
        let first = reader.utf16z().ok()?;
        let second = has_rate_and_value.then(|| reader.utf16z().ok()).flatten();
        let _instrument_id = reader.u32().ok()?;
        Some(Self {
            meter,
            instrument,
            tags,
            rate: (!first.is_empty()).then_some(first),
            value: second.filter(|value| !value.is_empty()),
        })
    }
}

fn generation_from_tags(tags: &str) -> Option<usize> {
    let value = tags
        .split(',')
        .find_map(|tag| tag.strip_prefix("gc.heap.generation="))?;
    GENERATION_TAGS
        .iter()
        .position(|generation| *generation == value)
}

fn parse_finite_f64(value: String) -> Option<f64> {
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_nonnegative_u64(value: String) -> Option<u64> {
    if let Ok(value) = value.parse::<u64>() {
        return Some(value);
    }
    parse_finite_f64(value).and_then(nonnegative_rounded_u64)
}

fn nonnegative_rounded_u64(value: f64) -> Option<u64> {
    (value >= 0.0 && value <= u64::MAX as f64).then(|| value.round() as u64)
}

struct NetTraceParser<R> {
    reader: CountingReader<R>,
    metadata: HashMap<u32, ParsedEvent>,
}

#[derive(Clone, Copy)]
enum ParsedEvent {
    Metrics(u32),
    RuntimeCounters,
}

impl<R: Read> NetTraceParser<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: CountingReader::new(reader),
            metadata: HashMap::new(),
        }
    }

    fn run(&mut self, mut on_event: impl FnMut(ParsedEvent, &[u8])) -> io::Result<()> {
        let mut magic = [0_u8; 8];
        self.reader.read_exact(&mut magic)?;
        if &magic != b"Nettrace" {
            return Err(invalid_data("unsupported EventPipe stream format"));
        }
        let header_len = read_u32(&mut self.reader)? as usize;
        if header_len > 64 {
            return Err(invalid_data("invalid FastSerialization header length"));
        }
        let mut header = vec![0; header_len];
        self.reader.read_exact(&mut header)?;
        if header != b"!FastSerialization.1" {
            return Err(invalid_data("invalid FastSerialization header"));
        }

        loop {
            let Some(tag) = read_optional_u8(&mut self.reader)? else {
                return Ok(());
            };
            if tag == 1 {
                return Ok(());
            }
            if tag != 5 || read_u8(&mut self.reader)? != 5 || read_u8(&mut self.reader)? != 1 {
                return Err(invalid_data("invalid FastSerialization object tags"));
            }
            let version = read_u32(&mut self.reader)?;
            let _minimum_reader_version = read_u32(&mut self.reader)?;
            let name_len = read_u32(&mut self.reader)? as usize;
            if name_len > MAX_TYPE_NAME_SIZE {
                return Err(invalid_data("invalid EventPipe block type length"));
            }
            let mut name = vec![0; name_len];
            self.reader.read_exact(&mut name)?;
            if read_u8(&mut self.reader)? != 6 {
                return Err(invalid_data("invalid EventPipe block type footer"));
            }

            if name == b"Trace" || name == b"Microsoft.DotNet.Runtime.EventPipeFile" {
                if version != 4 {
                    return Err(invalid_data("unsupported NetTrace version"));
                }
                let mut trace = [0_u8; 48];
                self.reader.read_exact(&mut trace)?;
            } else {
                let block_size = read_u32(&mut self.reader)? as usize;
                if block_size > MAX_BLOCK_SIZE {
                    return Err(invalid_data("EventPipe block exceeds the size limit"));
                }
                let padding = (4 - (self.reader.position() & 3)) & 3;
                let mut ignored = [0_u8; 3];
                self.reader.read_exact(&mut ignored[..padding])?;
                let mut block = vec![0; block_size];
                self.reader.read_exact(&mut block)?;
                if name == b"MetadataBlock" {
                    self.parse_metadata_block(&block)?;
                } else if name == b"EventBlock" {
                    self.parse_event_block(&block, &mut on_event)?;
                }
            }

            if read_u8(&mut self.reader)? != 6 {
                return Err(invalid_data("invalid EventPipe block footer"));
            }
        }
    }

    fn parse_metadata_block(&mut self, block: &[u8]) -> io::Result<()> {
        let mut reader = SliceReader::new(block);
        let header_size = usize::from(reader.u16()?);
        let flags = reader.u16()?;
        if header_size < 20 || header_size > block.len() {
            return Err(invalid_data("invalid metadata block header"));
        }
        reader.take(header_size - 4)?;
        let compressed = flags & 1 != 0;
        let mut previous = EventHeader::default();
        while reader.remaining() > 0 {
            read_event_header(&mut reader, compressed, &mut previous)?;
            let payload = reader.take(previous.payload_size)?;
            let mut metadata = SliceReader::new(payload);
            let metadata_id = metadata.u32()?;
            let provider = metadata.utf16z()?;
            let event_id = metadata.u32()?;
            let event_name = metadata.utf16z()?;
            if provider == METRICS_PROVIDER {
                self.metadata
                    .insert(metadata_id, ParsedEvent::Metrics(event_id));
            } else if provider == EVENT_COUNTER_PROVIDER && event_name == EVENT_COUNTER_NAME {
                self.metadata
                    .insert(metadata_id, ParsedEvent::RuntimeCounters);
            }
        }
        Ok(())
    }

    fn parse_event_block(
        &self,
        block: &[u8],
        on_event: &mut impl FnMut(ParsedEvent, &[u8]),
    ) -> io::Result<()> {
        let mut reader = SliceReader::new(block);
        let header_size = usize::from(reader.u16()?);
        let flags = reader.u16()?;
        if header_size < 20 || header_size > block.len() {
            return Err(invalid_data("invalid event block header"));
        }
        reader.take(header_size - 4)?;
        let compressed = flags & 1 != 0;
        let mut previous = EventHeader::default();
        while reader.remaining() > 0 {
            read_event_header(&mut reader, compressed, &mut previous)?;
            let payload = reader.take(previous.payload_size)?;
            if let Some(event) = self.metadata.get(&previous.metadata_id).copied() {
                on_event(event, payload);
            }
        }
        Ok(())
    }
}

struct CountingReader<R> {
    inner: R,
    position: usize,
}

impl<R> CountingReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, position: 0 }
    }

    fn position(&self) -> usize {
        self.position
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.position = self
            .position
            .checked_add(count)
            .ok_or_else(|| invalid_data("EventPipe stream position overflow"))?;
        Ok(count)
    }
}

#[derive(Clone, Copy, Default)]
struct EventHeader {
    metadata_id: u32,
    sequence: u32,
    capture_thread: u64,
    processor: u32,
    thread: u64,
    stack_id: u32,
    timestamp: u64,
    payload_size: usize,
}

fn read_event_header(
    reader: &mut SliceReader<'_>,
    compressed: bool,
    header: &mut EventHeader,
) -> io::Result<()> {
    if !compressed {
        let _event_size = reader.u32()?;
        header.metadata_id = reader.u32()? & 0x7fff_ffff;
        header.sequence = reader.u32()?;
        header.thread = reader.u64()?;
        header.capture_thread = reader.u64()?;
        header.processor = reader.u32()?;
        header.stack_id = reader.u32()?;
        header.timestamp = reader.u64()?;
        reader.take(32)?;
        header.payload_size = reader.u32()? as usize;
        return Ok(());
    }

    let flags = reader.u8()?;
    if flags & 1 != 0 {
        header.metadata_id = reader.var_u64()? as u32;
    }
    if flags & 2 != 0 {
        header.sequence = header
            .sequence
            .wrapping_add(reader.var_u64()? as u32)
            .wrapping_add(1);
        header.capture_thread = reader.var_u64()?;
        header.processor = reader.var_u64()? as u32;
    } else if header.metadata_id != 0 {
        header.sequence = header.sequence.wrapping_add(1);
    }
    if flags & 4 != 0 {
        header.thread = reader.var_u64()?;
    }
    if flags & 8 != 0 {
        header.stack_id = reader.var_u64()? as u32;
    }
    header.timestamp = header.timestamp.wrapping_add(reader.var_u64()?);
    if flags & 16 != 0 {
        reader.take(16)?;
    }
    if flags & 32 != 0 {
        reader.take(16)?;
    }
    if flags & 128 != 0 {
        header.payload_size = reader.var_u64()? as usize;
    }
    Ok(())
}

struct SliceReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, len: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| invalid_data("EventPipe length overflow"))?;
        if end > self.bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated EventPipe block",
            ));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("validated length"),
        ))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("validated length"),
        ))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("validated length"),
        ))
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("validated length"),
        ))
    }

    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("validated length"),
        ))
    }

    fn var_u64(&mut self) -> io::Result<u64> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = self.u8()?;
            if shift == 63 && byte & 0xfe != 0 {
                return Err(invalid_data("EventPipe variable-length integer overflow"));
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(invalid_data("invalid EventPipe variable-length integer"))
    }

    fn utf16z(&mut self) -> io::Result<String> {
        let mut units = Vec::new();
        loop {
            let unit = self.u16()?;
            if unit == 0 {
                break;
            }
            if units.len() >= 16 * 1024 {
                return Err(invalid_data("EventPipe string exceeds the size limit"));
            }
            units.push(unit);
        }
        String::from_utf16(&units).map_err(|_| invalid_data("invalid EventPipe UTF-16 string"))
    }
}

fn read_optional_u8(reader: &mut impl Read) -> io::Result<Option<u8>> {
    let mut value = [0_u8; 1];
    match reader.read(&mut value) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(value[0])),
        Err(error) => Err(error),
    }
}

fn read_u8(reader: &mut impl Read) -> io::Result<u8> {
    let mut value = [0_u8; 1];
    reader.read_exact(&mut value)?;
    Ok(value[0])
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut value = [0_u8; 4];
    reader.read_exact(&mut value)?;
    Ok(u32::from_le_bytes(value))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn verify_identity(identity: &ProcessIdentity) -> io::Result<()> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let Some(start) = identity.start_time else {
        return Err(io::Error::other("unknown process start time"));
    };
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(identity.pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    match system.process(Pid::from_u32(identity.pid)) {
        Some(process)
            if process.start_time() == start
                && process
                    .name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&identity.name) =>
        {
            Ok(())
        }
        _ => Err(io::Error::other("process exited or identity changed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_utf16z(buffer: &mut Vec<u8>, value: &str) {
        for unit in value.encode_utf16().chain(std::iter::once(0)) {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn metric_payload(instrument: &str, tags: &str, rate: &str, value: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        for field in [
            "SHARED",
            METRICS_METER,
            "10.0.0",
            instrument,
            "By",
            tags,
            rate,
            value,
        ] {
            push_utf16z(&mut payload, field);
        }
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload
    }

    fn polling_counter_payload(name: &str, value: f64, interval_sec: f32) -> Vec<u8> {
        let mut payload = Vec::new();
        push_utf16z(&mut payload, name);
        push_utf16z(&mut payload, name);
        payload.extend_from_slice(&value.to_le_bytes());
        payload.extend_from_slice(&0_f64.to_le_bytes());
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&value.to_le_bytes());
        payload.extend_from_slice(&value.to_le_bytes());
        payload.extend_from_slice(&interval_sec.to_le_bytes());
        push_utf16z(&mut payload, "Interval=1000");
        push_utf16z(&mut payload, "Mean");
        push_utf16z(&mut payload, "");
        push_utf16z(&mut payload, "MB");
        payload
    }

    fn incrementing_counter_payload(name: &str, value: f64, interval_sec: f32) -> Vec<u8> {
        let mut payload = Vec::new();
        push_utf16z(&mut payload, name);
        push_utf16z(&mut payload, name);
        push_utf16z(&mut payload, "00:00:01");
        payload.extend_from_slice(&value.to_le_bytes());
        payload.extend_from_slice(&interval_sec.to_le_bytes());
        push_utf16z(&mut payload, "Interval=1000");
        push_utf16z(&mut payload, "Sum");
        push_utf16z(&mut payload, "");
        push_utf16z(&mut payload, "B");
        payload
    }

    #[test]
    fn accumulator_publishes_complete_runtime_interval() {
        let mut accumulator = MetricAccumulator::default();
        accumulator.handle_event(EVENT_COLLECTION_START, &[]);
        accumulator.handle_event(
            EVENT_COUNTER_RATE,
            &metric_payload(GC_TOTAL_ALLOCATED, "", "4096", "8192"),
        );
        for (index, generation) in GENERATION_TAGS.iter().enumerate() {
            accumulator.handle_event(
                EVENT_UP_DOWN_COUNTER_RATE,
                &metric_payload(
                    GC_HEAP_SIZE,
                    &format!("gc.heap.generation={generation}"),
                    "0",
                    &(u64::try_from(index).unwrap() + 1).to_string(),
                ),
            );
            accumulator.handle_event(
                EVENT_UP_DOWN_COUNTER_RATE,
                &metric_payload(
                    GC_FRAGMENTATION,
                    &format!("gc.heap.generation={generation}"),
                    "0",
                    "2",
                ),
            );
        }
        accumulator.handle_event(
            EVENT_UP_DOWN_COUNTER_RATE,
            &metric_payload(GC_COMMITTED, "", "0", "100"),
        );

        let metrics = accumulator
            .handle_event(EVENT_COLLECTION_STOP, &[])
            .expect("complete interval");
        assert_eq!(metrics.heap_bytes, Some(15));
        assert_eq!(metrics.gen0_heap_bytes, Some(1));
        assert_eq!(metrics.gen1_heap_bytes, Some(2));
        assert_eq!(metrics.gen2_heap_bytes, Some(3));
        assert_eq!(metrics.loh_bytes, Some(4));
        assert_eq!(metrics.poh_bytes, Some(5));
        assert_eq!(metrics.committed_bytes, Some(100));
        assert_eq!(metrics.fragmentation_bytes, Some(10));
        assert_eq!(metrics.allocation_bytes_per_sec, Some(4096));
    }

    #[test]
    fn dropping_twenty_sessions_does_not_serialize_collector_shutdown_waits() {
        let mut sessions = Vec::new();
        let control = super::super::dotnet_control::DotNetControl::default();
        assert!(
            control.enable(
                (0..20)
                    .map(|pid| ProcessIdentity {
                        pid,
                        name: "test.exe".into(),
                        start_time: Some(1)
                    })
                    .collect()
            )
        );
        let mut release_txs = Vec::new();
        let mut finished_rxs = Vec::new();
        for pid in 0..20 {
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let (finished_tx, finished_rx) = std::sync::mpsc::channel();
            let join_handle = thread::spawn(move || {
                let _ = release_rx.recv();
                let _ = finished_tx.send(());
            });
            sessions.push(RuntimeSession {
                started_at: Instant::now(),
                permit: Arc::new(
                    control
                        .permit(&ProcessIdentity {
                            pid,
                            name: "test.exe".into(),
                            start_time: Some(1),
                        })
                        .unwrap(),
                ),
                identity: ProcessIdentity {
                    pid,
                    name: "test.exe".to_string(),
                    start_time: Some(1),
                },
                latest: Arc::new(Mutex::new(None)),
                session_id: Arc::new(AtomicU64::new(0)),
                stop_requested: Arc::new(AtomicBool::new(false)),
                stop_sent: Arc::new(AtomicBool::new(false)),
                finished: Arc::new(AtomicBool::new(false)),
                join_handle: Some(join_handle),
            });
            release_txs.push(release_tx);
            finished_rxs.push(finished_rx);
        }

        control.disable();
        let started = Instant::now();
        drop(sessions);
        control.finish_suspend();
        assert!(control.stopping());
        let elapsed = started.elapsed();
        for release_tx in release_txs {
            release_tx.send(()).unwrap();
        }
        for finished_rx in finished_rxs {
            finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }

        assert!(elapsed < Duration::from_millis(250), "elapsed: {elapsed:?}");
        let deadline = Instant::now() + Duration::from_secs(2);
        while control.stopping() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!control.stopping());
    }

    #[test]
    fn accumulator_does_not_undercount_incomplete_generation_series() {
        let mut accumulator = MetricAccumulator::default();
        accumulator.handle_event(EVENT_COLLECTION_START, &[]);
        accumulator.handle_event(
            EVENT_UP_DOWN_COUNTER_RATE,
            &metric_payload(GC_HEAP_SIZE, "gc.heap.generation=gen0", "0", "10"),
        );
        let metrics = accumulator
            .handle_event(EVENT_COLLECTION_STOP, &[])
            .expect("complete interval");
        assert_eq!(metrics.heap_bytes, None);
    }

    #[test]
    fn event_counter_accumulator_converts_complete_dotnet_8_interval() {
        let mut accumulator = EventCounterAccumulator::default();
        let events = [
            polling_counter_payload(EVENT_COUNTER_HEAP_SIZE, 12.5, 2.0),
            incrementing_counter_payload(EVENT_COUNTER_ALLOCATION_RATE, 8192.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_FRAGMENTATION, 20.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_COMMITTED, 16.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_GEN0_SIZE, 1_000.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_GEN1_SIZE, 2_000.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_GEN2_SIZE, 3_000.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_LOH_SIZE, 4_000.0, 2.0),
            polling_counter_payload(EVENT_COUNTER_POH_SIZE, 5_000.0, 2.0),
        ];
        let mut metrics = None;
        for event in events {
            metrics = accumulator.handle_event(&event).or(metrics);
        }
        let metrics = metrics.expect("complete EventCounters interval");
        assert_eq!(metrics.heap_bytes, Some(12_500_000));
        assert_eq!(metrics.gen0_heap_bytes, Some(1_000));
        assert_eq!(metrics.gen1_heap_bytes, Some(2_000));
        assert_eq!(metrics.gen2_heap_bytes, Some(3_000));
        assert_eq!(metrics.loh_bytes, Some(4_000));
        assert_eq!(metrics.poh_bytes, Some(5_000));
        assert_eq!(metrics.committed_bytes, Some(16_000_000));
        assert_eq!(metrics.fragmentation_bytes, Some(2_500_000));
        assert_eq!(metrics.allocation_bytes_per_sec, Some(4096));
    }

    #[test]
    fn event_counter_accumulator_does_not_mix_intervals() {
        let mut accumulator = EventCounterAccumulator::default();
        assert!(
            accumulator
                .handle_event(&polling_counter_payload(EVENT_COUNTER_HEAP_SIZE, 10.0, 1.0))
                .is_none()
        );
        assert!(
            accumulator
                .handle_event(&polling_counter_payload(EVENT_COUNTER_HEAP_SIZE, 20.0, 1.0))
                .is_none()
        );
        assert_eq!(accumulator.heap_bytes, Some(20_000_000));
        assert_eq!(accumulator.seen, 1);
    }

    #[test]
    fn collect_request_uses_shared_filtered_metrics_without_rundown_or_stacks() {
        let request = collect_tracing_request(42, CollectCommand::Tracing3);
        assert_eq!(&request[..14], IPC_MAGIC);
        assert_eq!(request[16], 2);
        assert_eq!(request[17], 4);
        assert_eq!(&request[20..24], &1_u32.to_le_bytes());
        assert_eq!(&request[24..28], &1_u32.to_le_bytes());
        assert_eq!(request[28], 0);
        assert_eq!(request[29], 0);
        assert!(contains_utf16(&request, "SessionId=SHARED"));
        assert!(contains_utf16(&request, GC_TOTAL_ALLOCATED));
        assert!(contains_utf16(&request, GC_COMMITTED));
        assert!(contains_utf16(&request, GC_HEAP_SIZE));
        assert!(contains_utf16(&request, GC_FRAGMENTATION));
        assert!(!contains_utf16(&request, "dotnet.gc.collections"));
        assert!(!contains_utf16(&request, "EventCounterIntervalSec=1"));
    }

    #[test]
    fn compatibility_request_uses_collect_tracing2_for_dotnet_8() {
        let request = collect_tracing_request(42, CollectCommand::Tracing2);
        assert_eq!(request[17], 3);
        assert_eq!(&request[20..24], &1_u32.to_le_bytes());
        assert_eq!(&request[24..28], &1_u32.to_le_bytes());
        assert_eq!(request[28], 0);
        assert_eq!(&request[29..33], &1_u32.to_le_bytes());
        assert!(contains_utf16(&request, EVENT_COUNTER_PROVIDER));
        assert!(contains_utf16(&request, "EventCounterIntervalSec=1"));
        assert!(!contains_utf16(&request, "SessionId=SHARED"));
    }

    #[test]
    fn unknown_ipc_command_is_classified_for_compatibility_collection() {
        let mut response = Vec::new();
        response.extend_from_slice(IPC_MAGIC);
        response.extend_from_slice(&24_u16.to_le_bytes());
        response.push(0xff);
        response.push(0xff);
        response.extend_from_slice(&0_u16.to_le_bytes());
        response.extend_from_slice(&IPC_UNKNOWN_COMMAND.to_le_bytes());

        let error = read_ipc_response(&mut std::io::Cursor::new(response)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn enabled_empty_scope_never_starts_sessions_for_live_processes() {
        let mut sampler = DotNetRuntimeSampler::new();
        assert!(sampler.control.enable(vec![]));
        let mut processes = vec![ProcessRow {
            pid: 42,
            name: "unapproved.exe".into(),
            start_time: Some(100),
            ..Default::default()
        }];
        sampler.reconcile_and_apply(&mut processes);
        assert!(sampler.control.enabled());
        assert!(sampler.sessions.is_empty());
        assert!(sampler.detected.is_empty());
        assert!(sampler.rejected.is_empty());
        for row in &processes {
            let identity = ProcessIdentity::from_row(row);
            assert!(!sampler.should_start(&identity));
            assert!(sampler.control.permit(&identity).is_none());
        }
    }

    #[test]
    fn first_metric_timeout_stops_waiting_without_retrying_or_blocking() {
        let mut sampler = DotNetRuntimeSampler::new();
        let row = ProcessRow {
            pid: 42,
            name: "silent.exe".into(),
            start_time: Some(100),
            ..Default::default()
        };
        let identity = ProcessIdentity::from_row(&row);
        assert!(sampler.control.enable_target(identity.clone()));
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut session = RuntimeSession {
            started_at: Instant::now(),
            permit: Arc::new(sampler.control.permit(&identity).unwrap()),
            identity: identity.clone(),
            latest: Arc::new(Mutex::new(None)),
            session_id: Arc::new(AtomicU64::new(0)),
            stop_requested: stop_requested.clone(),
            stop_sent: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            join_handle: None,
        };
        assert!(!session.first_metric_timed_out());
        session.started_at = Instant::now() - FIRST_METRIC_TIMEOUT - Duration::from_secs(1);
        assert!(session.first_metric_timed_out());
        // A session that has published once uses ordinary stale-data handling.
        *session.latest.lock().unwrap() = Some(PublishedMetrics {
            collected_at: Instant::now() - Duration::from_secs(20),
            metrics: DotNetRuntimeMetrics::default(),
        });
        assert!(!session.first_metric_timed_out());
        *session.latest.lock().unwrap() = None;
        sampler.sessions.insert(identity.clone(), session);
        let mut rows = vec![row];
        sampler.reconcile_and_apply(&mut rows);
        assert!(stop_requested.load(Ordering::Acquire));
        assert!(sampler.sessions.is_empty());
        assert!(sampler.rejected.contains(&identity));
        assert!(
            sampler
                .control
                .status(&identity)
                .contains("no .NET metrics received within 5s")
        );
        assert!(!sampler.control.is_starting(&identity));
        assert!(!sampler.control.is_collecting(&identity));
        assert!(!sampler.should_start(&identity));
        sampler.reconcile_and_apply(&mut rows);
        assert!(sampler.sessions.is_empty());
        assert!(rows[0].dotnet_heap_bytes.is_none());
    }

    #[test]
    fn negative_detection_is_cached_by_full_process_identity() {
        let mut sampler = DotNetRuntimeSampler::new();
        let identity = ProcessIdentity {
            pid: 42,
            name: "app.exe".to_string(),
            start_time: Some(100),
        };
        assert!(!sampler.should_start(&identity));
        assert!(sampler.control.enable(vec![identity.clone()]));
        assert!(sampler.should_start(&identity));
        sampler.rejected.insert(identity.clone());
        assert!(!sampler.should_start(&identity));

        let restarted = ProcessIdentity {
            start_time: Some(200),
            ..identity
        };
        assert!(!sampler.should_start(&restarted));
    }

    #[test]
    fn revoked_permission_writes_no_modern_or_fallback_request() {
        let pid = std::process::id();
        let system = sysinfo::System::new_all();
        let process = system.process(sysinfo::Pid::from_u32(pid)).unwrap();
        let identity = ProcessIdentity {
            pid,
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        let control = super::super::dotnet_control::DotNetControl::default();
        assert!(control.enable(vec![identity.clone()]));
        let permit = control.permit(&identity).unwrap();
        control.disable();
        for (i, command) in [CollectCommand::Tracing3, CollectCommand::Tracing2]
            .into_iter()
            .enumerate()
        {
            let path = std::env::temp_dir().join(format!("winproc-dotnet-revoked-{pid}-{i}.tmp"));
            let stream = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            let result = start_eventpipe_session(stream, &identity, &permit, command);
            assert!(result.is_err());
            assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn disabled_sampler_reconciles_without_attempting_detection() {
        let mut sampler = DotNetRuntimeSampler::new();
        let mut rows = vec![ProcessRow {
            pid: std::process::id(),
            name: "not-a-real-runtime".into(),
            start_time: Some(1),
            ..Default::default()
        }];
        sampler.reconcile_and_apply(&mut rows);
        assert!(sampler.sessions.is_empty());
        assert!(sampler.rejected.is_empty());
        assert!(sampler.detected.is_empty());
    }

    fn contains_utf16(bytes: &[u8], value: &str) -> bool {
        let needle = value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.windows(needle.len()).any(|window| window == needle)
    }
}
