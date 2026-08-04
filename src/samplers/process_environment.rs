use std::{
    mem::{size_of, transmute, zeroed},
    ptr::null_mut,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use chrono::Local;
use sysinfo::{Pid, System};
use winapi::{
    ctypes::c_void,
    shared::{
        minwindef::{FALSE, ULONG},
        ntdef::{HANDLE, LONG, PVOID},
        winerror::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
    },
    um::{
        errhandlingapi::GetLastError,
        handleapi::CloseHandle,
        libloaderapi::{GetModuleHandleW, GetProcAddress},
        memoryapi::{ReadProcessMemory, VirtualQueryEx},
        processthreadsapi::OpenProcess,
        winnt::{
            MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        },
        wow64apiset::IsWow64Process,
    },
};

use crate::model::{
    ProcessEnvironmentEntry, ProcessEnvironmentError, ProcessEnvironmentReport, ProcessIdentity,
    ProcessRow,
};

const MAX_ENVIRONMENT_BYTES: usize = 4 * 1024 * 1024;
const ENVIRONMENT_READ_CHUNK_BYTES: usize = 64 * 1024;
const PROCESS_BASIC_INFORMATION_CLASS: ULONG = 0;
const PROCESS_WOW64_INFORMATION_CLASS: ULONG = 26;
const PEB64_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
const PEB32_PROCESS_PARAMETERS_OFFSET: usize = 0x10;
const PROCESS_PARAMETERS64_ENVIRONMENT_OFFSET: usize = 0x80;
const PROCESS_PARAMETERS32_ENVIRONMENT_OFFSET: usize = 0x48;

type NtQueryInformationProcessFn = unsafe extern "system" fn(
    process_handle: HANDLE,
    process_information_class: ULONG,
    process_information: PVOID,
    process_information_length: ULONG,
    return_length: *mut ULONG,
) -> LONG;

#[derive(Debug, Clone)]
pub(crate) enum ProcessEnvironmentRequest {
    Collect {
        generation: u64,
        request_id: u64,
        identity: ProcessIdentity,
        process: ProcessRow,
    },
    Stop,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessEnvironmentResult {
    pub(crate) generation: u64,
    pub(crate) request_id: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) outcome: std::result::Result<ProcessEnvironmentReport, ProcessEnvironmentError>,
}

pub(crate) struct ProcessEnvironmentWorker {
    request_tx: Sender<ProcessEnvironmentRequest>,
    result_rx: Receiver<ProcessEnvironmentResult>,
    join_handle: Option<JoinHandle<()>>,
}

impl ProcessEnvironmentWorker {
    pub(crate) fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ProcessEnvironmentRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessEnvironmentResult>();
        let join_handle = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                match request {
                    ProcessEnvironmentRequest::Collect {
                        generation,
                        request_id,
                        identity,
                        process,
                    } => {
                        let outcome = collect_process_environment(&identity, &process);
                        if result_tx
                            .send(ProcessEnvironmentResult {
                                generation,
                                request_id,
                                identity,
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProcessEnvironmentRequest::Stop => break,
                }
            }
        });
        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }

    pub(crate) fn request_environment(
        &self,
        generation: u64,
        request_id: u64,
        identity: ProcessIdentity,
        process: ProcessRow,
    ) -> Result<()> {
        self.request_tx
            .send(ProcessEnvironmentRequest::Collect {
                generation,
                request_id,
                identity,
                process,
            })
            .context("process environment worker is unavailable")
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<ProcessEnvironmentResult, TryRecvError> {
        self.result_rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (
        Self,
        Receiver<ProcessEnvironmentRequest>,
        Sender<ProcessEnvironmentResult>,
    ) {
        let (request_tx, request_rx) = mpsc::channel::<ProcessEnvironmentRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessEnvironmentResult>();
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

    #[cfg(test)]
    pub(crate) fn test_noop() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ProcessEnvironmentRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessEnvironmentResult>();
        let join_handle = thread::spawn(move || {
            let _keep_result_channel_open = result_tx;
            while let Ok(request) = request_rx.recv() {
                if matches!(request, ProcessEnvironmentRequest::Stop) {
                    break;
                }
            }
        });
        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for ProcessEnvironmentWorker {
    fn drop(&mut self) {
        let _ = self.request_tx.send(ProcessEnvironmentRequest::Stop);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub(crate) fn collect_process_environment(
    identity: &ProcessIdentity,
    process: &ProcessRow,
) -> std::result::Result<ProcessEnvironmentReport, ProcessEnvironmentError> {
    if size_of::<usize>() != 8 {
        return Err(ProcessEnvironmentError::UnsupportedArchitecture);
    }
    verify_process_identity(identity)?;
    let handle = open_process(process.pid)?;
    let layout = remote_layout(handle.0)?;
    let peb = query_peb_address(handle.0, layout)?;
    let memory = ProcessMemory(handle.0);
    let environment = environment_pointer(&memory, layout, peb)?;
    let block = read_environment_block(handle.0, environment)?;
    let (entries, malformed_entries) = parse_environment_block(&block)?;
    verify_process_identity(identity)?;
    Ok(ProcessEnvironmentReport {
        pid: process.pid,
        process_name: process.name.clone(),
        captured_at: Local::now(),
        entries,
        malformed_entries,
    })
}

fn verify_process_identity(
    identity: &ProcessIdentity,
) -> std::result::Result<(), ProcessEnvironmentError> {
    let system = System::new_all();
    let Some(process) = system.process(Pid::from_u32(identity.pid)) else {
        return Err(ProcessEnvironmentError::ProcessExited);
    };
    if !process
        .name()
        .to_string_lossy()
        .eq_ignore_ascii_case(&identity.name)
        || identity
            .start_time
            .is_some_and(|start_time| process.start_time() != start_time)
    {
        return Err(ProcessEnvironmentError::IdentityChanged);
    }
    Ok(())
}

fn open_process(pid: u32) -> std::result::Result<OwnedHandle, ProcessEnvironmentError> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            FALSE,
            pid,
        )
    };
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        Err(match error {
            ERROR_ACCESS_DENIED => ProcessEnvironmentError::AccessDenied,
            ERROR_INVALID_PARAMETER => ProcessEnvironmentError::ProcessExited,
            _ => ProcessEnvironmentError::ReadFailed,
        })
    } else {
        Ok(OwnedHandle(handle))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteLayout {
    X64,
    Wow64,
}

fn remote_layout(handle: HANDLE) -> std::result::Result<RemoteLayout, ProcessEnvironmentError> {
    let mut wow64 = 0;
    if unsafe { IsWow64Process(handle, &mut wow64) } == 0 {
        return Err(ProcessEnvironmentError::UnsupportedArchitecture);
    }
    Ok(if wow64 != 0 {
        RemoteLayout::Wow64
    } else {
        RemoteLayout::X64
    })
}

#[repr(C)]
struct ProcessBasicInformation {
    reserved1: PVOID,
    peb_base_address: PVOID,
    reserved2: [PVOID; 2],
    unique_process_id: usize,
    reserved3: PVOID,
}

fn query_peb_address(
    handle: HANDLE,
    layout: RemoteLayout,
) -> std::result::Result<usize, ProcessEnvironmentError> {
    let query_information = resolve_nt_query_information_process()?;
    unsafe {
        match layout {
            RemoteLayout::X64 => {
                let mut info: ProcessBasicInformation = zeroed();
                let status = query_information(
                    handle,
                    PROCESS_BASIC_INFORMATION_CLASS,
                    &mut info as *mut _ as PVOID,
                    size_of::<ProcessBasicInformation>() as ULONG,
                    null_mut(),
                );
                if status < 0 || info.peb_base_address.is_null() {
                    Err(ProcessEnvironmentError::NotAvailable)
                } else {
                    Ok(info.peb_base_address as usize)
                }
            }
            RemoteLayout::Wow64 => {
                let mut peb = 0usize;
                let status = query_information(
                    handle,
                    PROCESS_WOW64_INFORMATION_CLASS,
                    &mut peb as *mut _ as PVOID,
                    size_of::<usize>() as ULONG,
                    null_mut(),
                );
                if status < 0 || peb == 0 {
                    Err(ProcessEnvironmentError::NotAvailable)
                } else {
                    Ok(peb)
                }
            }
        }
    }
}

fn resolve_nt_query_information_process()
-> std::result::Result<NtQueryInformationProcessFn, ProcessEnvironmentError> {
    const NTDLL: [u16; 10] = [
        b'n' as u16,
        b't' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        b'.' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        0,
    ];
    let module = unsafe { GetModuleHandleW(NTDLL.as_ptr()) };
    if module.is_null() {
        return Err(ProcessEnvironmentError::NotAvailable);
    }
    let address =
        unsafe { GetProcAddress(module, b"NtQueryInformationProcess\0".as_ptr().cast::<i8>()) };
    if address.is_null() {
        return Err(ProcessEnvironmentError::NotAvailable);
    }
    Ok(unsafe { transmute::<_, NtQueryInformationProcessFn>(address) })
}

trait RemoteMemory {
    fn read_exact(
        &self,
        address: usize,
        length: usize,
    ) -> std::result::Result<Vec<u8>, ProcessEnvironmentError>;
}

struct ProcessMemory(HANDLE);

impl RemoteMemory for ProcessMemory {
    fn read_exact(
        &self,
        address: usize,
        length: usize,
    ) -> std::result::Result<Vec<u8>, ProcessEnvironmentError> {
        let mut bytes = vec![0u8; length];
        let mut read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                self.0,
                address as *const c_void,
                bytes.as_mut_ptr() as *mut c_void,
                length,
                &mut read,
            )
        };
        if ok == 0 || read != length {
            Err(ProcessEnvironmentError::ReadFailed)
        } else {
            Ok(bytes)
        }
    }
}

fn environment_pointer(
    memory: &impl RemoteMemory,
    layout: RemoteLayout,
    peb: usize,
) -> std::result::Result<usize, ProcessEnvironmentError> {
    let (pointer_width, process_parameters_offset, environment_offset) = match layout {
        RemoteLayout::X64 => (
            8,
            PEB64_PROCESS_PARAMETERS_OFFSET,
            PROCESS_PARAMETERS64_ENVIRONMENT_OFFSET,
        ),
        RemoteLayout::Wow64 => (
            4,
            PEB32_PROCESS_PARAMETERS_OFFSET,
            PROCESS_PARAMETERS32_ENVIRONMENT_OFFSET,
        ),
    };
    let process_parameters_address = peb
        .checked_add(process_parameters_offset)
        .ok_or(ProcessEnvironmentError::ReadFailed)?;
    let process_parameters = read_pointer(memory, process_parameters_address, pointer_width)?;
    if process_parameters == 0 {
        return Err(ProcessEnvironmentError::NotAvailable);
    }
    let environment_address = process_parameters
        .checked_add(environment_offset)
        .ok_or(ProcessEnvironmentError::ReadFailed)?;
    let environment = read_pointer(memory, environment_address, pointer_width)?;
    if environment == 0 || environment % 2 != 0 {
        Err(ProcessEnvironmentError::NotAvailable)
    } else {
        Ok(environment)
    }
}

fn read_pointer(
    memory: &impl RemoteMemory,
    address: usize,
    width: usize,
) -> std::result::Result<usize, ProcessEnvironmentError> {
    let bytes = memory.read_exact(address, width)?;
    match width {
        4 => Ok(u32::from_le_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| ProcessEnvironmentError::ReadFailed)?,
        ) as usize),
        8 => Ok(u64::from_le_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| ProcessEnvironmentError::ReadFailed)?,
        ) as usize),
        _ => Err(ProcessEnvironmentError::UnsupportedArchitecture),
    }
}

fn read_environment_block(
    handle: HANDLE,
    environment: usize,
) -> std::result::Result<Vec<u8>, ProcessEnvironmentError> {
    let mut bytes = Vec::new();
    let mut current = environment;
    while bytes.len() < MAX_ENVIRONMENT_BYTES {
        let mut region: MEMORY_BASIC_INFORMATION = unsafe { zeroed() };
        let queried = unsafe {
            VirtualQueryEx(
                handle,
                current as *const c_void,
                &mut region,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if queried == 0
            || region.State != MEM_COMMIT
            || region.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0
        {
            return Err(ProcessEnvironmentError::ReadFailed);
        }
        let region_start = region.BaseAddress as usize;
        let region_end = region_start
            .checked_add(region.RegionSize)
            .ok_or(ProcessEnvironmentError::ReadFailed)?;
        if current < region_start || current >= region_end {
            return Err(ProcessEnvironmentError::ReadFailed);
        }
        let remaining_region = region_end - current;
        let remaining_limit = MAX_ENVIRONMENT_BYTES - bytes.len();
        let length = ENVIRONMENT_READ_CHUNK_BYTES
            .min(remaining_region)
            .min(remaining_limit)
            & !1usize;
        if length == 0 {
            return Err(ProcessEnvironmentError::ReadFailed);
        }
        let chunk = ProcessMemory(handle).read_exact(current, length)?;
        bytes.extend_from_slice(&chunk);
        if let Some(end) = environment_block_end(&bytes) {
            bytes.truncate(end);
            return Ok(bytes);
        }
        current = current
            .checked_add(length)
            .ok_or(ProcessEnvironmentError::ReadFailed)?;
    }
    Err(ProcessEnvironmentError::TooLarge)
}

fn environment_block_end(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return None;
    }
    let mut previous_null = false;
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        if value == 0 && previous_null {
            return Some((index + 1) * 2);
        }
        previous_null = value == 0;
    }
    None
}

fn parse_environment_block(
    bytes: &[u8],
) -> std::result::Result<(Vec<ProcessEnvironmentEntry>, usize), ProcessEnvironmentError> {
    if bytes.len() > MAX_ENVIRONMENT_BYTES || bytes.len() % 2 != 0 {
        return Err(if bytes.len() > MAX_ENVIRONMENT_BYTES {
            ProcessEnvironmentError::TooLarge
        } else {
            ProcessEnvironmentError::ReadFailed
        });
    }
    let end = environment_block_end(bytes).ok_or_else(|| {
        if bytes.len() >= MAX_ENVIRONMENT_BYTES {
            ProcessEnvironmentError::TooLarge
        } else {
            ProcessEnvironmentError::ReadFailed
        }
    })?;
    let units = bytes[..end]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let second_null = units
        .windows(2)
        .position(|pair| pair == [0, 0])
        .map(|index| index + 1)
        .ok_or(ProcessEnvironmentError::ReadFailed)?;
    let mut entries = Vec::new();
    let mut malformed = 0usize;
    for raw in units[..second_null].split(|value| *value == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = String::from_utf16(raw).map_err(|_| ProcessEnvironmentError::ReadFailed)?;
        match split_environment_entry(&text) {
            Some((name, value)) => entries.push(ProcessEnvironmentEntry {
                name: name.to_string(),
                value: value.to_string(),
            }),
            None => malformed += 1,
        }
    }
    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok((entries, malformed))
}

fn split_environment_entry(value: &str) -> Option<(&str, &str)> {
    let separator = if value.starts_with('=') {
        value
            .char_indices()
            .skip(1)
            .find_map(|(index, ch)| (ch == '=').then_some(index))?
    } else {
        value.find('=')?
    };
    let name = &value[..separator];
    (!name.is_empty()).then_some((name, &value[separator + 1..]))
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct FakeMemory {
        bytes: HashMap<usize, u8>,
    }

    impl FakeMemory {
        fn new() -> Self {
            Self {
                bytes: HashMap::new(),
            }
        }

        fn insert(&mut self, address: usize, bytes: &[u8]) {
            for (offset, byte) in bytes.iter().copied().enumerate() {
                self.bytes.insert(address + offset, byte);
            }
        }
    }

    impl RemoteMemory for FakeMemory {
        fn read_exact(
            &self,
            address: usize,
            length: usize,
        ) -> std::result::Result<Vec<u8>, ProcessEnvironmentError> {
            (0..length)
                .map(|offset| {
                    self.bytes
                        .get(&(address + offset))
                        .copied()
                        .ok_or(ProcessEnvironmentError::ReadFailed)
                })
                .collect()
        }
    }

    fn encode_block(entries: &[&str]) -> Vec<u8> {
        let mut units = Vec::new();
        for entry in entries {
            units.extend(entry.encode_utf16());
            units.push(0);
        }
        if entries.is_empty() {
            units.push(0);
        }
        units.push(0);
        units.into_iter().flat_map(u16::to_le_bytes).collect()
    }

    fn process_row(pid: u32, name: String, start_time: u64) -> ProcessRow {
        ProcessRow {
            pid,
            name,
            executable_path: None,
            start_time: Some(start_time),
            cpu_percent: None,
            private_bytes: None,
            workset_bytes: None,
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            workset_shared_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }

    #[test]
    fn x64_and_wow64_pointer_layouts_reach_environment_block() {
        let mut memory = FakeMemory::new();
        memory.insert(0x1020, &0x2000u64.to_le_bytes());
        memory.insert(0x2080, &0x3000u64.to_le_bytes());
        memory.insert(0x4010, &0x5000u32.to_le_bytes());
        memory.insert(0x5048, &0x6000u32.to_le_bytes());

        assert_eq!(
            environment_pointer(&memory, RemoteLayout::X64, 0x1000),
            Ok(0x3000)
        );
        assert_eq!(
            environment_pointer(&memory, RemoteLayout::Wow64, 0x4000),
            Ok(0x6000)
        );
    }

    #[test]
    fn null_overflow_and_partial_pointer_reads_are_errors() {
        let mut null_memory = FakeMemory::new();
        null_memory.insert(0x1020, &0u64.to_le_bytes());
        assert_eq!(
            environment_pointer(&null_memory, RemoteLayout::X64, 0x1000),
            Err(ProcessEnvironmentError::NotAvailable)
        );
        assert_eq!(
            environment_pointer(&FakeMemory::new(), RemoteLayout::X64, usize::MAX - 1),
            Err(ProcessEnvironmentError::ReadFailed)
        );
        assert_eq!(
            environment_pointer(&FakeMemory::new(), RemoteLayout::Wow64, 0x1000),
            Err(ProcessEnvironmentError::ReadFailed)
        );
    }

    #[test]
    fn environment_parser_handles_drive_entries_empty_values_and_malformed_rows() {
        let block = encode_block(&[
            "Path=C:\\Windows",
            "PATH=D:\\Other",
            "=C:=C:\\work",
            "EMPTY=",
            "BROKEN",
        ]);
        let (entries, malformed) = parse_environment_block(&block).unwrap();

        assert_eq!(malformed, 1);
        assert_eq!(entries[0].name, "=C:");
        assert_eq!(entries[0].value, r"C:\work");
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.eq_ignore_ascii_case("path"))
                .map(|entry| entry.value.as_str())
                .collect::<Vec<_>>(),
            vec![r"C:\Windows", r"D:\Other"]
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.name == "EMPTY")
                .unwrap()
                .value,
            ""
        );
    }

    #[test]
    fn empty_environment_block_is_valid() {
        let (entries, malformed) = parse_environment_block(&encode_block(&[])).unwrap();
        assert!(entries.is_empty());
        assert_eq!(malformed, 0);
    }

    #[test]
    fn invalid_utf16_and_unterminated_large_blocks_are_rejected() {
        let invalid = vec![0x00, 0xd8, 0, 0, 0, 0];
        assert_eq!(
            parse_environment_block(&invalid),
            Err(ProcessEnvironmentError::ReadFailed)
        );
        assert_eq!(
            parse_environment_block(&[b'A']),
            Err(ProcessEnvironmentError::ReadFailed)
        );
        let too_large = vec![b'A'; MAX_ENVIRONMENT_BYTES];
        assert_eq!(
            parse_environment_block(&too_large),
            Err(ProcessEnvironmentError::TooLarge)
        );
    }

    #[test]
    fn current_process_environment_is_collectable_without_elevation() {
        let pid = std::process::id();
        let system = System::new_all();
        let current = system
            .process(Pid::from_u32(pid))
            .expect("test process should be visible");
        let name = current.name().to_string_lossy().into_owned();
        let process = process_row(pid, name.clone(), current.start_time());
        let identity = ProcessIdentity {
            pid,
            name,
            start_time: Some(current.start_time()),
        };

        let report = collect_process_environment(&identity, &process)
            .expect("the current process environment should be readable");
        assert!(!report.entries.is_empty());
        assert!(report.entries.iter().all(|entry| !entry.name.is_empty()));
    }
}
