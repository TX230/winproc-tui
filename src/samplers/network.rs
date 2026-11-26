use jiff::Timestamp;
use std::{
    collections::HashMap,
    mem::{size_of, zeroed},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    path::Path,
    ptr::read_unaligned,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Result, anyhow};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use winapi::{
    shared::{
        iprtrmib::{TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID},
        minwindef::{FALSE, FILETIME},
        ntdef::HANDLE,
        tcpmib::{MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID},
        udpmib::{MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID},
        winerror::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        ws2def::{AF_INET, AF_INET6},
    },
    um::{
        handleapi::CloseHandle,
        iphlpapi::{GetExtendedTcpTable, GetExtendedUdpTable},
        processthreadsapi::{GetProcessTimes, OpenProcess},
        winbase::QueryFullProcessImageNameW,
        winnt::PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

use crate::model::{
    ProcessIdentity,
    network::{NetworkEndpoint, NetworkEndpointKey, NetworkOwner, NetworkProtocol, NetworkReport},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetworkContext {
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) target: Option<ProcessIdentity>,
}

#[derive(Debug, Clone)]
pub(crate) enum NetworkRequest {
    Collect(NetworkContext),
    Verify(NetworkContext, NetworkOwner),
}

#[derive(Debug)]
pub(crate) enum NetworkPayload {
    Report(std::result::Result<NetworkReport, String>),
    Owner(std::result::Result<NetworkOwner, String>),
}

#[derive(Debug)]
pub(crate) struct NetworkResult {
    pub(crate) context: NetworkContext,
    pub(crate) payload: NetworkPayload,
}

pub(crate) struct NetworkWorker {
    tx: Option<SyncSender<NetworkRequest>>,
    rx: Receiver<NetworkResult>,
    join: Option<JoinHandle<()>>,
}

impl NetworkWorker {
    pub(crate) fn spawn() -> Self {
        let (tx, requests) = mpsc::sync_channel(2);
        let (results, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                let (context, payload) = match request {
                    NetworkRequest::Collect(context) => {
                        let report =
                            collect_endpoints(context.target.as_ref()).map_err(|e| e.to_string());
                        (context, NetworkPayload::Report(report))
                    }
                    NetworkRequest::Verify(context, owner) => {
                        let result = verify_owner(&owner)
                            .map(|()| owner)
                            .map_err(|e| e.to_string());
                        (context, NetworkPayload::Owner(result))
                    }
                };
                if results.send(NetworkResult { context, payload }).is_err() {
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            rx,
            join: Some(join),
        }
    }

    pub(crate) fn request(&self, request: NetworkRequest) -> Result<()> {
        self.tx
            .as_ref()
            .ok_or_else(|| anyhow!("Network worker unavailable"))?
            .try_send(request)
            .map_err(|_| anyhow!("Network worker busy or unavailable"))
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<NetworkResult, TryRecvError> {
        self.rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, Receiver<NetworkRequest>, mpsc::Sender<NetworkResult>) {
        let (tx, requests) = mpsc::sync_channel(2);
        let (results, rx) = mpsc::channel();
        (
            Self {
                tx: Some(tx),
                rx,
                join: None,
            },
            requests,
            results,
        )
    }
}

impl Drop for NetworkWorker {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(pid: u32) -> Result<Self> {
        // SAFETY: no pointers are passed. Only a non-null owned process handle is retained.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) };
        if handle.is_null() {
            return Err(anyhow!("Process unavailable or access denied"));
        }
        Ok(Self(handle))
    }

    fn live_creation_time(&self) -> Result<u64> {
        // SAFETY: FILETIME is a plain integer pair; zero is a valid initialization.
        let mut times: [FILETIME; 4] = unsafe { zeroed() };
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: the process handle is live and each output references a distinct writable FILETIME.
        if unsafe { GetProcessTimes(self.0, created, exited, kernel, user) } == 0 {
            return Err(anyhow!("Process timing unavailable"));
        }
        if filetime(exited) != 0 {
            return Err(anyhow!("Process exited"));
        }
        Ok(filetime(created))
    }

    fn image_path(&self) -> Result<String> {
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        // SAFETY: the held process handle is valid; the initialized UTF-16 buffer and length
        // are separate writable outputs, and its capacity is provided in WCHAR units.
        if unsafe { QueryFullProcessImageNameW(self.0, 0, buffer.as_mut_ptr(), &mut length) } == 0
            || length as usize > buffer.len()
        {
            return Err(anyhow!("Process image unavailable"));
        }
        Ok(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: this owner holds one successful OpenProcess result and closes it exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn filetime(time: &FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}
fn unix_seconds(time: u64) -> Option<u64> {
    time.checked_sub(116_444_736_000_000_000)
        .map(|t| t / 10_000_000)
}

fn verify_owner(owner: &NetworkOwner) -> Result<()> {
    let handle = ProcessHandle::open(owner.identity.pid)?;
    if handle.live_creation_time()? != owner.creation_time {
        return Err(anyhow!("Process identity changed"));
    }
    Ok(())
}

pub(crate) fn collect_endpoints(target: Option<&ProcessIdentity>) -> Result<NetworkReport> {
    let started_at = Timestamp::now();
    let pids = if let Some(target) = target {
        vec![target.pid]
    } else {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        system.processes().keys().map(|pid| pid.as_u32()).collect()
    };
    let mut owners = HashMap::new();
    for pid in pids {
        let Ok(handle) = ProcessHandle::open(pid) else {
            continue;
        };
        let Ok(creation_time) = handle.live_creation_time() else {
            continue;
        };
        let start_time = unix_seconds(creation_time);
        let Ok(image_path) = handle.image_path() else {
            continue;
        };
        let Some(name) = Path::new(&image_path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if target.is_some_and(|target| {
            target.start_time != start_time || !target.name.eq_ignore_ascii_case(&name)
        }) {
            continue;
        }
        let owner = NetworkOwner {
            identity: ProcessIdentity {
                pid,
                name,
                start_time,
            },
            executable_path: Some(image_path),
            creation_time,
        };
        owners.insert(pid, (owner, handle));
    }
    if target.is_some_and(|target| !owners.contains_key(&target.pid)) {
        return Err(anyhow!("Target exited, changed, or could not be verified"));
    }
    let mut report = NetworkReport {
        started_at,
        captured_at: started_at,
        endpoints: Vec::new(),
        failures: Vec::new(),
        successful_tables: 0,
    };
    for protocol in NetworkProtocol::ALL {
        match query_table(protocol).and_then(|bytes| parse_table(protocol, &bytes)) {
            Ok(mut endpoints) => {
                report.successful_tables += 1;
                report.endpoints.append(&mut endpoints);
            }
            Err(error) => report
                .failures
                .push(format!("{}: {error}", protocol.label())),
        }
    }
    // Handles were acquired before table capture. Retain owners only if that same process
    // lifetime survived the capture, rather than attaching a later name to a recycled PID.
    owners
        .retain(|_, (owner, handle)| handle.live_creation_time().ok() == Some(owner.creation_time));
    if target.is_some_and(|target| !owners.contains_key(&target.pid)) {
        return Err(anyhow!("Target exited during network collection"));
    }
    report
        .endpoints
        .retain(|entry| target.is_none_or(|target| entry.key.pid == target.pid));
    for endpoint in &mut report.endpoints {
        endpoint.owner = owners
            .get(&endpoint.key.pid)
            .map(|(owner, _)| owner.clone());
    }
    report.endpoints.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.tcp_state.cmp(&b.tcp_state))
    });
    report.captured_at = Timestamp::now();
    Ok(report)
}

fn query_table(protocol: NetworkProtocol) -> Result<Vec<u8>> {
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    let ipv6 = matches!(protocol, NetworkProtocol::Tcp6 | NetworkProtocol::Udp6);
    let family = if ipv6 { AF_INET6 } else { AF_INET } as u32;
    let mut size = 16 * 1024usize;
    for _ in 0..6 {
        if size > MAX_BYTES {
            return Err(anyhow!("Table size limit exceeded"));
        }
        // Every OWNER_PID row has at most DWORD alignment; words provide that alignment.
        let mut words = vec![0u32; size.div_ceil(4)];
        let mut bytes = (words.len() * 4) as u32;
        // SAFETY: the aligned buffer is writable for `bytes`; bytes is an independent output.
        // The family and table class select only the DWORD-aligned OWNER_PID table layouts.
        let status = unsafe {
            let buffer = words.as_mut_ptr().cast();
            match protocol {
                NetworkProtocol::Tcp4 | NetworkProtocol::Tcp6 => GetExtendedTcpTable(
                    buffer,
                    &mut bytes,
                    FALSE,
                    family,
                    TCP_TABLE_OWNER_PID_ALL,
                    0,
                ),
                NetworkProtocol::Udp4 | NetworkProtocol::Udp6 => {
                    GetExtendedUdpTable(buffer, &mut bytes, FALSE, family, UDP_TABLE_OWNER_PID, 0)
                }
            }
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            size = (bytes as usize).max(size * 2);
            continue;
        }
        if status != ERROR_SUCCESS {
            return Err(anyhow!("Windows error {status}"));
        }
        if bytes as usize > words.len() * 4 {
            return Err(anyhow!("Invalid table length"));
        }
        // SAFETY: the checked byte count is within the initialized allocation, which remains live.
        return Ok(unsafe {
            std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), bytes as usize)
        }
        .to_vec());
    }
    Err(anyhow!("Table kept changing; refresh again"))
}

fn port(value: u32) -> u16 {
    u16::from_be(value as u16)
}
fn v4(address: u32, raw_port: u32) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::from(address.to_ne_bytes()),
        port(raw_port),
    ))
}
fn v6(address: [u8; 16], scope: u32, raw_port: u32) -> SocketAddr {
    // IP Helper OWNER_PID tables return native host-order interface scopes on Windows 11.
    // Verified against bound TCP/UDP link-local sockets; only the port needs a byte swap.
    SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::from(address),
        port(raw_port),
        0,
        scope,
    ))
}

fn parse_table(protocol: NetworkProtocol, bytes: &[u8]) -> Result<Vec<NetworkEndpoint>> {
    let header: [u8; 4] = bytes
        .get(..4)
        .ok_or_else(|| anyhow!("Short table header"))?
        .try_into()?;
    let count = u32::from_ne_bytes(header) as usize;
    let row_size = match protocol {
        NetworkProtocol::Tcp4 => size_of::<MIB_TCPROW_OWNER_PID>(),
        NetworkProtocol::Tcp6 => size_of::<MIB_TCP6ROW_OWNER_PID>(),
        NetworkProtocol::Udp4 => size_of::<MIB_UDPROW_OWNER_PID>(),
        NetworkProtocol::Udp6 => size_of::<MIB_UDP6ROW_OWNER_PID>(),
    };
    let required = count
        .checked_mul(row_size)
        .and_then(|n| n.checked_add(4))
        .ok_or_else(|| anyhow!("Table length overflow"))?;
    if required > bytes.len() {
        return Err(anyhow!("Short table body"));
    }
    let mut endpoints = Vec::with_capacity(count);
    for row in bytes[4..required].chunks_exact(row_size) {
        // SAFETY: each chunk contains a whole selected ABI row. These structures contain only
        // integers/byte arrays, so every bit pattern is valid; unaligned reads do not borrow storage.
        let (local, remote, state, pid) = unsafe {
            match protocol {
                NetworkProtocol::Tcp4 => {
                    let r = read_unaligned(row.as_ptr().cast::<MIB_TCPROW_OWNER_PID>());
                    (
                        v4(r.dwLocalAddr, r.dwLocalPort),
                        (r.dwState != 2).then(|| v4(r.dwRemoteAddr, r.dwRemotePort)),
                        Some(r.dwState),
                        r.dwOwningPid,
                    )
                }
                NetworkProtocol::Tcp6 => {
                    let r = read_unaligned(row.as_ptr().cast::<MIB_TCP6ROW_OWNER_PID>());
                    (
                        v6(r.ucLocalAddr, r.dwLocalScopeId, r.dwLocalPort),
                        (r.dwState != 2)
                            .then(|| v6(r.ucRemoteAddr, r.dwRemoteScopeId, r.dwRemotePort)),
                        Some(r.dwState),
                        r.dwOwningPid,
                    )
                }
                NetworkProtocol::Udp4 => {
                    let r = read_unaligned(row.as_ptr().cast::<MIB_UDPROW_OWNER_PID>());
                    (v4(r.dwLocalAddr, r.dwLocalPort), None, None, r.dwOwningPid)
                }
                NetworkProtocol::Udp6 => {
                    let r = read_unaligned(row.as_ptr().cast::<MIB_UDP6ROW_OWNER_PID>());
                    (
                        v6(r.ucLocalAddr, r.dwLocalScopeId, r.dwLocalPort),
                        None,
                        None,
                        r.dwOwningPid,
                    )
                }
            }
        };
        endpoints.push(NetworkEndpoint {
            key: NetworkEndpointKey {
                protocol,
                local,
                remote,
                pid,
            },
            tcp_state: state,
            owner: None,
        });
    }
    Ok(endpoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream, UdpSocket};

    fn words(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }

    #[test]
    fn network_parser_decodes_ipv4_ports_and_tcp_listener_remote() {
        let bytes = [
            1, 0, 0, 0, // one row
            2, 0, 0, 0, // Listen
            127, 0, 0, 1, 0x12, 0x34, 0, 0, 192, 0, 2, 1, 0x01, 0xbb, 0, 0, 42, 0, 0, 0,
        ];
        let mut entry = parse_table(NetworkProtocol::Tcp4, &bytes)
            .unwrap()
            .remove(0);
        assert_eq!(entry.key.local, "127.0.0.1:4660".parse().unwrap());
        assert_eq!(entry.key.pid, 42);
        assert_eq!(entry.key.remote, None);
        assert_eq!(entry.state_label(), "LISTENING");
        let mut connected = bytes;
        connected[4] = 5;
        entry = parse_table(NetworkProtocol::Tcp4, &connected)
            .unwrap()
            .remove(0);
        assert_eq!(entry.key.remote, Some("192.0.2.1:443".parse().unwrap()));
        assert!(!entry.is_listener_or_udp());
        let udp = [1, 0, 0, 0, 0, 0, 0, 0, 0x14, 0xe9, 0, 0, 42, 0, 0, 0];
        entry = parse_table(NetworkProtocol::Udp4, &udp).unwrap().remove(0);
        assert_eq!(entry.key.local, "0.0.0.0:5353".parse().unwrap());
        assert_eq!(entry.state_label(), "");
        assert!(entry.is_listener_or_udp());
    }

    #[test]
    fn network_parser_preserves_ipv6_scopes_and_rejects_truncated_tables() {
        let address = "fe80::1234".parse::<Ipv6Addr>().unwrap().octets();
        let mut udp = words(&[1]);
        udp.extend(address);
        udp.extend(words(&[7, u32::from(5353u16.to_be()), 42]));
        let entry = parse_table(NetworkProtocol::Udp6, &udp).unwrap().remove(0);
        assert_eq!(entry.key.local.to_string(), "[fe80::1234%7]:5353");
        let mut tcp = words(&[1]);
        tcp.extend(address);
        tcp.extend(words(&[7, u32::from(12345u16.to_be())]));
        tcp.extend("fe80::5678".parse::<Ipv6Addr>().unwrap().octets());
        tcp.extend(words(&[9, u32::from(443u16.to_be()), 5, 42]));
        let entry = parse_table(NetworkProtocol::Tcp6, &tcp).unwrap().remove(0);
        assert_eq!(entry.key.local.to_string(), "[fe80::1234%7]:12345");
        assert_eq!(entry.key.remote.unwrap().to_string(), "[fe80::5678%9]:443");
        for protocol in NetworkProtocol::ALL {
            assert!(parse_table(protocol, &[]).is_err());
            assert!(parse_table(protocol, &[1, 0, 0, 0]).is_err());
            assert!(parse_table(protocol, &[255, 255, 255, 255]).is_err());
            assert!(parse_table(protocol, &[0, 0, 0, 0]).unwrap().is_empty());
        }
        tcp.pop();
        assert!(parse_table(NetworkProtocol::Tcp6, &tcp).is_err());
    }

    #[test]
    fn network_native_loopback_tables_match_live_sockets_and_owner_lifetime() {
        let tcp4 = TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp6 = TcpListener::bind("[::1]:0").unwrap();
        let udp4 = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp6 = UdpSocket::bind("[::1]:0").unwrap();
        let client4 = TcpStream::connect(tcp4.local_addr().unwrap()).unwrap();
        let client6 = TcpStream::connect(tcp6.local_addr().unwrap()).unwrap();
        let (_server4, remote4) = tcp4.accept().unwrap();
        let (_server6, remote6) = tcp6.accept().unwrap();
        let report = collect_endpoints(None).unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.successful_tables, 4);
        let pid = std::process::id();
        for (protocol, local, state) in [
            (NetworkProtocol::Tcp4, tcp4.local_addr().unwrap(), Some(2)),
            (NetworkProtocol::Tcp6, tcp6.local_addr().unwrap(), Some(2)),
            (NetworkProtocol::Udp4, udp4.local_addr().unwrap(), None),
            (NetworkProtocol::Udp6, udp6.local_addr().unwrap(), None),
        ] {
            let entry = report
                .endpoints
                .iter()
                .find(|entry| {
                    entry.key.protocol == protocol
                        && entry.key.local == local
                        && entry.tcp_state == state
                })
                .expect("bound socket row");
            assert_eq!(entry.key.pid, pid);
            let owner = entry.owner.as_ref().expect("verified current process");
            assert_eq!(owner.identity.pid, pid);
            verify_owner(owner).unwrap();
        }
        for (client, remote) in [(&client4, remote4), (&client6, remote6)] {
            assert_eq!(client.local_addr().unwrap(), remote);
            let entry = report
                .endpoints
                .iter()
                .find(|entry| entry.key.local == remote && entry.tcp_state == Some(5))
                .expect("established client");
            assert_eq!(entry.key.remote, Some(client.peer_addr().unwrap()));
        }
        let owner = report
            .endpoints
            .iter()
            .find(|entry| entry.key.local == udp4.local_addr().unwrap())
            .unwrap()
            .owner
            .as_ref()
            .unwrap();
        let targeted = collect_endpoints(Some(&owner.identity())).unwrap();
        assert!(targeted.endpoints.iter().all(|entry| entry.key.pid == pid));
        let mut wrong_owner = owner.clone();
        wrong_owner.creation_time += 1;
        assert!(verify_owner(&wrong_owner).is_err());
        let mut wrong_identity = owner.identity();
        wrong_identity.start_time = Some(0);
        assert!(collect_endpoints(Some(&wrong_identity)).is_err());
    }
}
