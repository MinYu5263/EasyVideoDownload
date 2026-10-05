use std::io;
use tokio::process::{Child, Command};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        collections::BTreeMap,
        mem::{size_of, zeroed},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        sync::Mutex,
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{
                GetProcessIdOfThread, OpenThread, ResumeThread, SuspendThread, CREATE_NO_WINDOW,
                CREATE_SUSPENDED, THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
            },
        },
    };

    // The job owns the full process tree and kills it when this handle is dropped.
    pub struct ProcessTree(OwnedHandle, Mutex<BTreeMap<u32, OwnedHandle>>);

    fn open_thread_for_pause(thread_id: u32, process_id: u32) -> io::Result<Option<OwnedHandle>> {
        // SAFETY: open the thread from the private job's enumeration without inheriting it.
        let handle = unsafe {
            OpenThread(THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION, 0, thread_id)
        };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            // ToolHelp is a snapshot: a thread can exit before we open it. Other
            // errors still fail the operation and roll back earlier suspensions.
            return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let thread = unsafe { OwnedHandle::from_raw_handle(handle) };
        // A stale ID can also be reused. Validate the owner on the live handle
        // before touching it, rather than relying only on the earlier snapshot.
        let owner = unsafe { GetProcessIdOfThread(handle) };
        if owner == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((owner == process_id).then_some(thread))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

        #[test]
        fn pause_ignores_a_thread_that_exited_after_enumeration() {
            let thread_id = std::thread::spawn(|| unsafe { GetCurrentThreadId() })
                .join()
                .unwrap();
            let owner = unsafe { GetCurrentProcessId() };
            // The stale ID normally disappears, but another concurrent test may
            // already have reused it. Either a safe handle or None is legitimate.
            let _thread = open_thread_for_pause(thread_id, owner).unwrap();
            assert!(open_thread_for_pause(0, owner).unwrap().is_none());
        }

        #[test]
        fn pause_does_not_open_a_reused_thread_id_from_another_process() {
            let thread_id = unsafe { GetCurrentThreadId() };
            assert!(open_thread_for_pause(thread_id, 0).unwrap().is_none());
            assert!(open_thread_for_pause(thread_id, unsafe { GetCurrentProcessId() })
                .unwrap()
                .is_some());
        }
    }

    impl ProcessTree {
        pub fn set_paused(&self, paused: bool) -> io::Result<()> {
            let mut suspended = self.1.lock().map_err(|e| io::Error::other(e.to_string()))?;
            if !paused {
                // Keep failed handles so a subsequent resume can retry them.
                let mut failure = None;
                suspended.retain(|_, thread| {
                    // SAFETY: these owned handles were suspended exactly once by this tree.
                    if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
                        failure = Some(io::Error::last_os_error());
                        true
                    } else {
                        false
                    }
                });
                return failure.map_or(Ok(()), Err);
            }
            if !suspended.is_empty() {
                return Ok(());
            }
            let result = (|| {
                // Repeat after suspending to catch threads/children created during enumeration.
                for _ in 0..8 {
                    #[repr(C)]
                    struct ProcessIds {
                        assigned: u32,
                        listed: u32,
                        ids: [usize; 1024],
                    }
                    let mut processes = ProcessIds {
                        assigned: 0,
                        listed: 0,
                        ids: [0; 1024],
                    };
                    // SAFETY: the aligned buffer matches JOBOBJECT_BASIC_PROCESS_ID_LIST.
                    if unsafe {
                        QueryInformationJobObject(
                            self.0.as_raw_handle() as HANDLE,
                            JobObjectBasicProcessIdList,
                            (&mut processes as *mut ProcessIds).cast(),
                            size_of::<ProcessIds>() as u32,
                            std::ptr::null_mut(),
                        )
                    } == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    // SAFETY: this snapshot only reads thread IDs.
                    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
                    if snapshot == INVALID_HANDLE_VALUE {
                        return Err(io::Error::last_os_error());
                    }
                    // SAFETY: the snapshot is newly owned.
                    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
                    let mut entry: THREADENTRY32 = unsafe { zeroed() };
                    entry.dwSize = size_of::<THREADENTRY32>() as u32;
                    let mut found =
                        unsafe { Thread32First(snapshot.as_raw_handle() as HANDLE, &mut entry) }
                            != 0;
                    let mut added = false;
                    while found {
                        if processes.ids[..processes.listed as usize]
                            .contains(&(entry.th32OwnerProcessID as usize))
                            && !suspended.contains_key(&entry.th32ThreadID)
                        {
                            if let Some(thread) = open_thread_for_pause(
                                entry.th32ThreadID, entry.th32OwnerProcessID,
                            )? {
                                if unsafe { SuspendThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
                                    return Err(io::Error::other(format!(
                                        "SuspendThread during pause: {}", io::Error::last_os_error()
                                    )));
                                }
                                suspended.insert(entry.th32ThreadID, thread);
                                added = true;
                            }
                        }
                        found =
                            unsafe { Thread32Next(snapshot.as_raw_handle() as HANDLE, &mut entry) }
                                != 0;
                    }
                    if !added {
                        return if suspended.is_empty() {
                            Err(io::Error::other("download process already stopped"))
                        } else {
                            Ok(())
                        };
                    }
                }
                Err(io::Error::other(
                    "download process tree changed during pause",
                ))
            })();
            if result.is_err() {
                for thread in suspended.values() {
                    // SAFETY: rollback the single suspension applied above.
                    unsafe {
                        ResumeThread(thread.as_raw_handle() as HANDLE);
                    }
                }
                suspended.clear();
            }
            result
        }

        fn new() -> io::Result<Self> {
            // SAFETY: null pointers use default security and an unnamed job.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateJobObjectW returned a new handle owned by this value.
            let job = Self(
                unsafe { OwnedHandle::from_raw_handle(handle) },
                Mutex::default(),
            );
            // SAFETY: this Windows POD structure permits zero initialization.
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the live handle and information buffer have the required type/size.
            if unsafe {
                SetInformationJobObject(
                    job.0.as_raw_handle() as HANDLE,
                    JobObjectExtendedLimitInformation,
                    (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        fn attach_and_resume(&self, child: &Child) -> io::Result<()> {
            let handle = child
                .raw_handle()
                .ok_or_else(|| io::Error::other("missing process handle"))?;
            // SAFETY: both handles remain live; the child was created suspended.
            if unsafe {
                AssignProcessToJobObject(self.0.as_raw_handle() as HANDLE, handle as HANDLE)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let pid = child
                .id()
                .ok_or_else(|| io::Error::other("missing process id"))?;
            // std/Tokio do not expose the primary thread handle. While suspended, the
            // child has only its primary thread, so locate that thread by its owner PID.
            // SAFETY: the snapshot includes thread IDs only and is scoped to this call.
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: the returned snapshot is a newly owned handle.
            let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
            // SAFETY: THREADENTRY32 permits zero initialization followed by dwSize.
            let mut entry: THREADENTRY32 = unsafe { zeroed() };
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            // SAFETY: entry is a correctly sized writable buffer; snapshot is live.
            let mut found =
                unsafe { Thread32First(snapshot.as_raw_handle() as HANDLE, &mut entry) } != 0;
            while found {
                if entry.th32OwnerProcessID == pid {
                    // SAFETY: open only the suspended child thread; do not inherit the handle.
                    let thread =
                        unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                    if thread.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    // SAFETY: OpenThread returned a newly owned handle.
                    let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
                    // SAFETY: this is the primary thread suspended by our CREATE_SUSPENDED flag.
                    if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok(());
                }
                // SAFETY: the buffer and snapshot remain valid for the next entry.
                found =
                    unsafe { Thread32Next(snapshot.as_raw_handle() as HANDLE, &mut entry) } != 0;
            }
            Err(io::Error::other("could not find suspended child thread"))
        }
    }

    pub async fn spawn(command: &mut Command) -> io::Result<(Child, ProcessTree)> {
        let tree = ProcessTree::new()?;
        // Attach before any user code executes, preventing descendants escaping the job.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        if let Err(error) = tree.attach_and_resume(&child) {
            drop(tree);
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(error);
        }
        Ok((child, tree))
    }
}

#[cfg(windows)]
pub use windows::{spawn, ProcessTree};

#[cfg(unix)]
pub struct ProcessTree(i32);

#[cfg(unix)]
impl ProcessTree {
    pub fn set_paused(&self, paused: bool) -> io::Result<()> {
        // SAFETY: the negative ID targets only the private process group we created.
        if unsafe { libc::kill(-self.0, if paused { libc::SIGSTOP } else { libc::SIGCONT }) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // SAFETY: this ID is the private process group created for our child.
        unsafe {
            libc::kill(-self.0, libc::SIGKILL);
        }
    }
}

#[cfg(unix)]
pub async fn spawn(command: &mut Command) -> io::Result<(Child, ProcessTree)> {
    command.process_group(0);
    let child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("missing process id"))?;
    Ok((child, ProcessTree(pid as i32)))
}
