use std::io;

use winapi::um::{
    processthreadsapi::{GetCurrentThread, SetThreadInformation, SetThreadPriority},
    winbase::THREAD_PRIORITY_ABOVE_NORMAL,
};

// Windows SDK definitions missing from winapi 0.3.9.
const THREAD_POWER_THROTTLING: u32 = 3;
const POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
const EXECUTION_SPEED: u32 = 1;

#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
struct PowerThrottlingState {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

pub(super) fn configure_current_thread() -> Option<String> {
    // Evaluate both requests even if one fails; neither failure stops sampling.
    let priority = raise_current_thread_priority();
    let qos = request_current_thread_high_qos();
    configuration_warning(priority, qos)
}

fn configuration_warning(priority: io::Result<()>, qos: io::Result<()>) -> Option<String> {
    let failures = [
        priority
            .err()
            .map(|error| format!("Could not raise sampling thread priority: {error}")),
        qos.err()
            .map(|error| format!("Could not request sampling thread HighQoS: {error}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    (!failures.is_empty()).then(|| failures.join(" | "))
}

fn request_current_thread_high_qos() -> io::Result<()> {
    let mut policy = PowerThrottlingState {
        version: POWER_THROTTLING_CURRENT_VERSION,
        control_mask: EXECUTION_SPEED,
        state_mask: 0,
    };
    // SAFETY: the pseudo-handle selects only this worker. The DWORD-aligned C
    // structure, version, class and size match the Windows SDK. Clearing the
    // selected execution-speed mechanism requests HighQoS, not CPU affinity.
    if unsafe {
        SetThreadInformation(
            GetCurrentThread(),
            THREAD_POWER_THROTTLING,
            (&mut policy as *mut PowerThrottlingState).cast(),
            std::mem::size_of::<PowerThrottlingState>() as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn raise_current_thread_priority() -> io::Result<()> {
    // SAFETY: GetCurrentThread returns the calling worker's pseudo-handle. The
    // documented relative priority changes only that thread and needs no close.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL as i32) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::um::processthreadsapi::{
        GetCurrentProcess, GetProcessInformation, GetThreadPriority,
    };

    fn current_priority() -> i32 {
        // SAFETY: the pseudo-handle identifies this live calling thread.
        unsafe { GetThreadPriority(GetCurrentThread()) }
    }

    #[test]
    fn native_scheduling_requests_are_limited_to_the_worker() {
        let caller_priority = current_priority();
        let process_policy = current_process_policy();
        std::thread::spawn(|| {
            assert_eq!(configure_current_thread(), None);
            assert_eq!(current_priority(), THREAD_PRIORITY_ABOVE_NORMAL as i32);
        })
        .join()
        .expect("priority probe thread");
        assert_eq!(current_priority(), caller_priority);
        assert_eq!(current_process_policy(), process_policy);
    }

    fn current_process_policy() -> PowerThrottlingState {
        let mut policy = PowerThrottlingState {
            version: POWER_THROTTLING_CURRENT_VERSION,
            control_mask: 0,
            state_mask: 0,
        };
        // SAFETY: live current-process pseudo-handle, correctly sized C layout;
        // ProcessPowerThrottling (4) uses the same three DWORD fields as the
        // thread policy. This query does not modify process or thread state.
        assert_ne!(
            unsafe {
                GetProcessInformation(
                    GetCurrentProcess(),
                    4,
                    (&mut policy as *mut PowerThrottlingState).cast(),
                    std::mem::size_of::<PowerThrottlingState>() as u32,
                )
            },
            0
        );
        policy
    }

    #[test]
    fn configuration_reports_either_or_both_failures() {
        assert_eq!(configuration_warning(Ok(()), Ok(())), None);
        let failure = || Err(io::Error::from_raw_os_error(5));
        let priority = configuration_warning(failure(), Ok(())).unwrap();
        assert!(priority.contains("priority"));
        assert!(!priority.contains("HighQoS"));
        let qos = configuration_warning(Ok(()), failure()).unwrap();
        assert!(qos.contains("HighQoS"));
        assert!(!qos.contains("priority"));
        assert_eq!(
            configuration_warning(failure(), failure()),
            Some(format!("{priority} | {qos}"))
        );
    }
}
