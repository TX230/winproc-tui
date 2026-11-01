//! Local named-pipe transport. The application consumes a bounded request queue.
use super::{
    protocol::{self, IPC_MAJOR, IPC_MINOR, Message},
    security::{self, Identity},
};
use serde_json::{Value, json};
use std::{io, mem, os::windows::io::AsRawHandle, ptr, sync::mpsc, thread, time::Duration};
use tokio::{
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
    sync::oneshot,
    task::JoinSet,
    time::timeout,
};
use winapi::{
    shared::sddl::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    um::{
        minwinbase::SECURITY_ATTRIBUTES,
        winbase::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, LocalFree},
    },
};

pub(crate) struct Request {
    pub(crate) connection: String,
    pub(crate) method: String,
    pub(crate) arguments: Value,
    pub(crate) reply: oneshot::Sender<Value>,
}

pub(crate) enum Event {
    Ready(String),
    Failed(String),
    Connected(String),
    Disconnected(String),
}

pub(crate) struct Server {
    pub(crate) requests: mpsc::Receiver<Request>,
    pub(crate) events: mpsc::Receiver<Event>,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
impl Server {
    pub(crate) fn start() -> io::Result<Self> {
        let identity = security::current_identity()?;
        let (request_tx, requests) = mpsc::sync_channel(32);
        let (event_tx, events) = mpsc::sync_channel(64);
        let (stop, stopped) = oneshot::channel();
        thread::Builder::new()
            .name("mcp-ipc".into())
            .spawn(move || {
                let result = (|| -> io::Result<()> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        tokio::select! {
                            result = serve(identity, request_tx, event_tx.clone()) => result,
                            _ = stopped => Ok(()),
                        }
                    })
                })();
                if result.is_err() {
                    let _ = event_tx.send(Event::Failed("MCP listener unavailable".into()));
                }
            })?;
        Ok(Self {
            requests,
            events,
            stop: Some(stop),
        })
    }
}

fn listener(identity: &Identity, first: bool) -> io::Result<NamedPipeServer> {
    let sddl = security::wide(&format!("D:P(A;;GA;;;{})", identity.logon_sid));
    let mut descriptor = ptr::null_mut();
    // SAFETY: terminated UTF-16 input, valid output pointer. Windows allocates descriptor.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: attributes and its descriptor stay alive until creation returns.
    let result = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(5)
            .create_with_security_attributes_raw(
                security::pipe_name(identity),
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    };
    // SAFETY: descriptor was allocated by the conversion API and is no longer referenced.
    unsafe {
        LocalFree(descriptor);
    }
    result
}

async fn serve(
    identity: Identity,
    requests: mpsc::SyncSender<Request>,
    events: mpsc::SyncSender<Event>,
) -> io::Result<()> {
    let instance = uuid::Uuid::new_v4().to_string();
    let mut pipe = listener(&identity, true)?;
    let _ = events.send(Event::Ready(instance.clone()));
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = clients.join_next(), if !clients.is_empty() => {},
            connected = pipe.connect(), if clients.len() < 4 => {
                connected?;
                let next = listener(&identity, false)?;
                let connected = mem::replace(&mut pipe, next);
                let identity = identity.clone(); let instance = instance.clone();
                let requests = requests.clone(); let events = events.clone();
                clients.spawn(async move {
                    let connection = uuid::Uuid::new_v4().to_string();
                    let _ = session(connected, identity, instance, connection.clone(), requests, events.clone()).await;
                    let _ = events.send(Event::Disconnected(connection));
                });
            }
        }
    }
}

async fn session(
    mut pipe: NamedPipeServer,
    identity: Identity,
    instance: String,
    connection: String,
    requests: mpsc::SyncSender<Request>,
    events: mpsc::SyncSender<Event>,
) -> io::Result<()> {
    let mut pid = 0;
    // SAFETY: pipe is connected and owns the handle; pid is writable.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    security::verify_process(pid, &identity)?;
    let hello = timeout(Duration::from_secs(2), protocol::read_frame(&mut pipe)).await??;
    if !matches!(
        hello,
        Message::Hello {
            ipc_major: IPC_MAJOR,
            ..
        }
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC version mismatch",
        ));
    }
    protocol::write_frame(
        &mut pipe,
        &Message::Welcome {
            ipc_major: IPC_MAJOR,
            ipc_minor: IPC_MINOR,
            backend_instance_id: instance,
            application_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;
    let _ = events.send(Event::Connected(connection.clone()));
    let raw_pipe = pipe.as_raw_handle() as usize;
    let (mut reader, mut writer) = tokio::io::split(pipe);
    // Closing an overlapped handle can defer final destruction while buffered I/O
    // remains pending. Explicit disconnect releases a peer blocked in WriteFile.
    struct DisconnectGuard(usize);
    impl Drop for DisconnectGuard {
        fn drop(&mut self) {
            // SAFETY: declared after the split halves, so the writer still owns
            // the connected handle when this guard runs, including cancellation.
            unsafe {
                winapi::um::namedpipeapi::DisconnectNamedPipe(self.0 as _);
            }
        }
    }
    let _disconnect = DisconnectGuard(raw_pipe);
    let (incoming, mut messages) = tokio::sync::mpsc::channel(8);
    let reader = tokio::spawn(async move {
        loop {
            let message = protocol::read_frame(&mut reader).await;
            let failed = message.is_err();
            if incoming.send(message).await.is_err() || failed {
                break;
            }
        }
    });
    // Abort the reader whenever the session ends, including a writer timeout.
    struct ReaderGuard(tokio::task::AbortHandle);
    impl Drop for ReaderGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _reader = ReaderGuard(reader.abort_handle());
    let mut pending = std::collections::HashMap::new();
    let mut responses = JoinSet::new();
    loop {
        tokio::select! {
            message = messages.recv() => {
                let message = message.ok_or_else(||io::Error::from(io::ErrorKind::UnexpectedEof))??;
                match message {
                    Message::Cancel {request_id} => { if let Some(task) = pending.remove(&request_id) { tokio::task::AbortHandle::abort(&task); } },
                    Message::Request {request_id,method,arguments} => {
                        if request_id.is_empty() || request_id.len()>128 || pending.contains_key(&request_id) { return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid request ID")); }
                        if pending.len()>=8 {
                            timeout(Duration::from_secs(10),protocol::write_frame(&mut writer,&Message::Response{request_id,result:error("BUSY","In-flight request limit reached",true)})).await??;
                            continue;
                        }
                        let (reply,result)=oneshot::channel();
                        let request=Request{connection:connection.clone(),method,arguments,reply};
                        let busy=requests.try_send(request).is_err();
                        let id=request_id.clone();
                        let task=responses.spawn(async move {
                            let response=if busy {error("BUSY","MCP request queue is full",true)} else {
                                match timeout(Duration::from_secs(10),result).await {Ok(Ok(value))=>value,_=>error("TIMEOUT","MCP request did not complete",true)}
                            };
                            (id,response)
                        });
                        pending.insert(request_id,task);
                    },
                    _ => return Err(io::Error::new(io::ErrorKind::InvalidData,"expected request or cancellation")),
                }
            },
            response = responses.join_next(), if !responses.is_empty() => {
                if let Some(Ok((request_id,result)))=response {
                    pending.remove(&request_id);
                    timeout(Duration::from_secs(10),protocol::write_frame(&mut writer,&Message::Response{request_id,result})).await??;
                }
            }
        }
    }
}

pub(crate) fn error(code: &str, message: &str, retryable: bool) -> Value {
    json!({"error":{"code":code,"message":message,"retryable":retryable}})
}

pub(crate) struct Client {
    pipe: NamedPipeClient,
    pub(crate) instance: String,
}
impl Client {
    pub(crate) async fn connect() -> io::Result<Self> {
        let identity = security::current_identity()?;
        let name = security::pipe_name(&identity);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut pipe = loop {
            match ClientOptions::new().open(&name) {
                Ok(pipe) => break pipe,
                Err(error)
                    if error.raw_os_error()
                        == Some(winapi::shared::winerror::ERROR_PIPE_BUSY as i32)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let mut pid = 0;
        // SAFETY: connected handle is valid and pid points to writable storage.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        security::verify_process(pid, &identity)?;
        protocol::write_frame(
            &mut pipe,
            &Message::Hello {
                ipc_major: IPC_MAJOR,
                ipc_minor: IPC_MINOR,
                bridge_version: env!("CARGO_PKG_VERSION").into(),
            },
        )
        .await?;
        match protocol::read_frame(&mut pipe).await? {
            Message::Welcome {
                ipc_major: IPC_MAJOR,
                backend_instance_id,
                ..
            } => Ok(Self {
                pipe,
                instance: backend_instance_id,
            }),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "IPC version mismatch",
            )),
        }
    }
    pub(crate) async fn call(&mut self, method: &str, arguments: Value) -> io::Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        protocol::write_frame(
            &mut self.pipe,
            &Message::Request {
                request_id: id.clone(),
                method: method.into(),
                arguments,
            },
        )
        .await?;
        match protocol::read_frame(&mut self.pipe).await? {
            Message::Response { request_id, result }
                if request_id == id
                    && result.get("backend_instance_id").is_none_or(|instance| {
                        instance.as_str() == Some(self.instance.as_str())
                    }) =>
            {
                Ok(result)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected IPC response",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses the current logon session's MCP endpoint; run with the TUI stopped"]
    fn native_pipe_round_trip_and_endpoint_squatting() {
        let server = Server::start().unwrap();
        let instance = match server.events.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Ready(instance) => instance,
            _ => panic!("listener did not start"),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let identity = security::current_identity().unwrap();
            assert!(listener(&identity, true).is_err());
            let responder = thread::spawn(move || {
                let request = server
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                assert_eq!(request.method, "test");
                request
                    .reply
                    .send(json!({"echo":request.arguments}))
                    .unwrap();
                thread::sleep(Duration::from_millis(100));
            });
            let (first, second, third, fourth) = timeout(Duration::from_secs(3), async {
                tokio::join!(
                    Client::connect(),
                    Client::connect(),
                    Client::connect(),
                    Client::connect()
                )
            })
            .await
            .unwrap();
            let mut client = first.unwrap();
            let peers = [second.unwrap(), third.unwrap(), fourth.unwrap()];
            assert_eq!(client.instance, instance);
            let reply = client
                .call("test", json!({"value":"verified"}))
                .await
                .unwrap();
            assert_eq!(reply, json!({"echo":{"value":"verified"}}));
            drop(client);
            drop(peers);
            responder.join().unwrap();
        });
    }
}

#[cfg(test)]
mod connection_indicator_tests {
    use super::*;

    // A unique real pipe avoids interfering with a running user's TUI endpoint.
    #[test]
    fn native_connection_event_requires_a_successful_authenticated_handshake() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for valid in [false, true] {
                let name = format!(r"\\.\pipe\winproc-badge-test-{}", uuid::Uuid::new_v4());
                let pipe = ServerOptions::new()
                    .first_pipe_instance(true)
                    .reject_remote_clients(true)
                    .create(&name)
                    .unwrap();
                let mut client = ClientOptions::new().open(&name).unwrap();
                pipe.connect().await.unwrap();
                let (requests, _rx) = mpsc::sync_channel(2);
                let (events, rx) = mpsc::sync_channel(8);
                let task = tokio::spawn(session(
                    pipe,
                    security::current_identity().unwrap(),
                    "test".into(),
                    "client".into(),
                    requests,
                    events,
                ));
                tokio::task::yield_now().await;
                assert!(
                    rx.try_recv().is_err(),
                    "a raw pipe connection must not light the badge"
                );
                protocol::write_frame(
                    &mut client,
                    &Message::Hello {
                        bridge_version: "test".into(),
                        ipc_major: if valid { IPC_MAJOR } else { IPC_MAJOR + 1 },
                        ipc_minor: IPC_MINOR,
                    },
                )
                .await
                .unwrap();
                if valid {
                    assert!(matches!(
                        protocol::read_frame(&mut client).await.unwrap(),
                        Message::Welcome { .. }
                    ));
                    timeout(Duration::from_secs(2), async {
                        loop {
                            if let Ok(event) = rx.try_recv() {
                                assert!(matches!(event, Event::Connected(id) if id == "client"));
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    })
                    .await
                    .unwrap();
                    drop(client);
                }
                assert!(
                    timeout(Duration::from_secs(3), task)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_err()
                );
                assert!(
                    rx.try_recv().is_err(),
                    "no connection event for a rejected handshake"
                );
            }
        });
    }
}
