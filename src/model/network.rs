use std::net::SocketAddr;

use chrono::{DateTime, Local};

use super::{ProcessIdentity, ProcessRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum NetworkProtocol {
    Tcp4,
    Tcp6,
    Udp4,
    Udp6,
}

impl NetworkProtocol {
    pub(crate) const ALL: [Self; 4] = [Self::Tcp4, Self::Tcp6, Self::Udp4, Self::Udp6];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tcp4 => "TCP4",
            Self::Tcp6 => "TCP6",
            Self::Udp4 => "UDP4",
            Self::Udp6 => "UDP6",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkOwner {
    pub(crate) identity: ProcessIdentity,
    pub(crate) executable_path: Option<String>,
    // Native FILETIME precision is retained for navigation, independently of history timestamps.
    pub(crate) creation_time: u64,
}

impl NetworkOwner {
    pub(crate) fn identity(&self) -> ProcessIdentity {
        self.identity.clone()
    }

    pub(crate) fn process_row(&self) -> ProcessRow {
        ProcessRow {
            pid: self.identity.pid,
            name: self.identity.name.clone(),
            start_time: self.identity.start_time,
            executable_path: self.executable_path.clone(),
            ..ProcessRow::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NetworkEndpointKey {
    pub(crate) protocol: NetworkProtocol,
    pub(crate) local: SocketAddr,
    pub(crate) remote: Option<SocketAddr>,
    pub(crate) pid: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkEndpoint {
    pub(crate) key: NetworkEndpointKey,
    pub(crate) tcp_state: Option<u32>,
    pub(crate) owner: Option<NetworkOwner>,
}

impl NetworkEndpoint {
    pub(crate) fn is_listener_or_udp(&self) -> bool {
        self.tcp_state.is_none_or(|state| state == 2)
    }

    pub(crate) fn state_label(&self) -> &'static str {
        match self.tcp_state {
            None => "",
            Some(1) => "CLOSED",
            Some(2) => "LISTENING",
            Some(3) => "SYN_SENT",
            Some(4) => "SYN_RECEIVED",
            Some(5) => "ESTABLISHED",
            Some(6) => "FIN_WAIT_1",
            Some(7) => "FIN_WAIT_2",
            Some(8) => "CLOSE_WAIT",
            Some(9) => "CLOSING",
            Some(10) => "LAST_ACK",
            Some(11) => "TIME_WAIT",
            Some(12) => "DELETE_TCB",
            Some(_) => "UNKNOWN",
        }
    }

    pub(crate) fn process_name(&self) -> &str {
        self.owner
            .as_ref()
            .map_or("--", |owner| owner.identity.name.as_str())
    }

    pub(crate) fn plain_text(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.key.protocol.label(),
            self.key.local,
            self.key
                .remote
                .map_or_else(|| "--".into(), |remote| remote.to_string()),
            self.state_label(),
            self.key.pid,
            self.process_name()
        )
    }

    pub(crate) fn matches(&self, filter: &str) -> bool {
        let text = self.plain_text().to_lowercase();
        super::search::terms(filter)
            .iter()
            .all(|term| text.contains(term))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkReport {
    pub(crate) started_at: DateTime<Local>,
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) endpoints: Vec<NetworkEndpoint>,
    pub(crate) failures: Vec<String>,
    pub(crate) successful_tables: usize,
}
