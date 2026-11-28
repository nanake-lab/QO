use std::{
    io,
    process::{Child, Command},
};

pub struct Tree {
    child: Child,
    killed: bool,
    #[cfg(target_os = "linux")]
    _parent_thread: std::sync::mpsc::Sender<()>,
    #[cfg(windows)]
    _job: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
mod windows {
    use super::Tree;
    use std::{
        io,
        mem::{size_of, zeroed},
        os::windows::{
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        process::Command,
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
    };

    fn handle(raw: HANDLE) -> io::Result<OwnedHandle> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
        }
    }
    pub fn spawn(cmd: &mut Command) -> io::Result<Tree> {
        // Start suspended: no user code or descendants can run before job assignment.
        unsafe {
            let job = handle(CreateJobObjectW(std::ptr::null(), std::ptr::null()))?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut child = cmd
                .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
                .spawn()?;
            if AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0 {
                let error = io::Error::last_os_error();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            let tree = Tree {
                child,
                _job: job,
                killed: false,
            };
            let snapshot = handle(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0))?;
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut ok = Thread32First(snapshot.as_raw_handle(), &mut entry);
            while ok != 0 {
                if entry.th32OwnerProcessID == tree.child.id() {
                    let thread = handle(OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID))?;
                    if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok(tree);
                }
                ok = Thread32Next(snapshot.as_raw_handle(), &mut entry);
            }
            Err(io::Error::other("missing suspended process thread"))
        }
    }
}
impl Tree {
    pub fn spawn(mut cmd: Command) -> io::Result<Self> {
        #[cfg(windows)]
        {
            windows::spawn(&mut cmd)
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
            #[cfg(target_os = "linux")]
            unsafe {
                let parent = libc::getpid();
                cmd.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(io::Error::from_raw_os_error(libc::ESRCH));
                    }
                    Ok(())
                });
            }
            #[cfg(target_os = "linux")]
            {
                // PDEATHSIG watches the creating thread. A blocking-pool worker can retire early.
                let (ready, result) = std::sync::mpsc::sync_channel(1);
                let (parent_thread, lifetime) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("codex-parent".into())
                    .spawn(move || {
                        if ready.send(cmd.spawn()).is_ok() {
                            let _ = lifetime.recv();
                        }
                    })?;
                let child = result
                    .recv()
                    .map_err(|_| io::Error::other("Codex launcher stopped"))??;
                Ok(Self {
                    child,
                    killed: false,
                    _parent_thread: parent_thread,
                })
            }
            #[cfg(not(target_os = "linux"))]
            {
                Ok(Self {
                    child: cmd.spawn()?,
                    killed: false,
                })
            }
        }
    }
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }
    pub fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            use std::os::windows::io::AsRawHandle;
            windows_sys::Win32::System::JobObjects::TerminateJobObject(
                self._job.as_raw_handle(),
                1,
            );
        }
        let _ = self.child.wait();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}
