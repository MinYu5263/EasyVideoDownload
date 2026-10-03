use std::io;
use tokio::process::{Child, Command};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{
                OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
            },
        },
    };

    // The job owns the full process tree and kills it when this handle is dropped.
    pub struct ProcessTree(OwnedHandle);

    impl ProcessTree {
        fn new() -> io::Result<Self> {
            // SAFETY: null pointers use default security and an unnamed job.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateJobObjectW returned a new handle owned by this value.
            let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
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
pub use windows::spawn;

#[cfg(unix)]
pub struct ProcessTree(i32);

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
