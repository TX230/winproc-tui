use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};

use super::super::open_files::{OwnedHandle, final_path_for_handle};

const PATH_WAIT: Duration = Duration::from_millis(100);
const MAX_WORKERS: usize = 4;

// Only duplicated disk handles are transferred, with exactly one owner and one close.
struct DiskHandle(OwnedHandle);
// SAFETY: a duplicated Windows file handle is valid across threads in the same process.
unsafe impl Send for DiskHandle {}

pub(super) enum PathLookup {
    Ready(Option<String>),
    Expired,
    Busy,
}

struct Worker {
    requests: Sender<DiskHandle>,
    replies: Receiver<Option<String>>,
    awaiting_late_reply: bool,
}

impl Worker {
    fn spawn(resolve: impl Fn(DiskHandle) -> Option<String> + Send + 'static) -> Result<Self> {
        let (requests, inputs) = mpsc::channel();
        let (outputs, replies) = mpsc::channel();
        // A stuck native call belongs to the disposable helper. Never join it on exit.
        thread::Builder::new()
            .name("file-path".into())
            .spawn(move || {
                for handle in inputs {
                    if outputs.send(resolve(handle)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests,
            replies,
            awaiting_late_reply: false,
        })
    }

    fn available(&mut self) -> Result<bool> {
        if self.awaiting_late_reply {
            match self.replies.try_recv() {
                Ok(_) => self.awaiting_late_reply = false,
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => return Err(anyhow!("Path worker exited")),
            }
        }
        Ok(true)
    }
}

#[derive(Default)]
pub(super) struct PathWorkers {
    workers: Vec<Worker>,
}

impl PathWorkers {
    pub(super) fn lookup(&mut self, handle: OwnedHandle) -> Result<PathLookup> {
        let mut index = self.workers.len();
        for (i, worker) in self.workers.iter_mut().enumerate() {
            if worker.available()? {
                index = i;
                break;
            }
        }
        if index == self.workers.len() {
            if index == MAX_WORKERS {
                return Ok(PathLookup::Busy);
            }
            self.workers.push(Worker::spawn(|DiskHandle(handle)| {
                final_path_for_handle(handle.0)
            })?);
        }
        let worker = &mut self.workers[index];
        worker
            .requests
            .send(DiskHandle(handle))
            .map_err(|_| anyhow!("Path worker exited"))?;
        match worker.replies.recv_timeout(PATH_WAIT) {
            Ok(path) => Ok(PathLookup::Ready(path)),
            Err(RecvTimeoutError::Timeout) => {
                worker.awaiting_late_reply = true;
                Ok(PathLookup::Expired)
            }
            Err(RecvTimeoutError::Disconnected) => Err(anyhow!("Path worker exited")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, os::windows::io::IntoRawHandle, time::Instant};

    fn disk_handle() -> OwnedHandle {
        let file = File::open(std::env::current_exe().unwrap()).unwrap();
        OwnedHandle(file.into_raw_handle().cast())
    }

    #[test]
    fn file_users_continues_with_a_real_path_after_a_timeout() {
        let (_release_on_drop, gate) = mpsc::channel::<String>();
        let mut pool = PathWorkers::default();
        pool.workers
            .push(Worker::spawn(move |_| gate.recv().ok()).unwrap());
        assert!(matches!(
            pool.lookup(disk_handle()).unwrap(),
            PathLookup::Expired
        ));
        assert!(
            matches!(pool.lookup(disk_handle()).unwrap(), PathLookup::Ready(Some(path))
            if path.ends_with(std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap()))
        );
    }

    #[test]
    fn file_users_blocked_paths_do_not_consume_later_replies() {
        let mut pool = PathWorkers::default();
        let mut gates = Vec::new();
        for _ in 0..MAX_WORKERS {
            let (gate, release) = mpsc::channel();
            gates.push(gate);
            pool.workers
                .push(Worker::spawn(move |_| release.recv().ok()).unwrap());
        }
        for _ in 0..MAX_WORKERS {
            assert!(matches!(
                pool.lookup(disk_handle()).unwrap(),
                PathLookup::Expired
            ));
        }
        assert!(matches!(
            pool.lookup(disk_handle()).unwrap(),
            PathLookup::Busy
        ));

        gates[0].send("late".to_string()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pool.workers[0].available().unwrap() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        gates[0].send("next".to_string()).unwrap();
        assert!(
            matches!(pool.lookup(disk_handle()).unwrap(), PathLookup::Ready(Some(path)) if path == "next")
        );
    }
}
