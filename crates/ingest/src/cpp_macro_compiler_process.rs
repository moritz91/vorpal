//! A compiler driver owns its subprocess tree, including timeout/error cleanup.
use std::process::Child;

#[cfg(unix)]
pub(super) struct ProcessTree(u32);

#[cfg(unix)]
impl ProcessTree {
  pub(super) fn attach(child: &Child) -> Option<Self> {
    Some(Self(child.id()))
  }
}

#[cfg(unix)]
impl Drop for ProcessTree {
  fn drop(&mut self) {
    // The command created this dedicated process group before exec. A completed
    // driver must not leave preprocessing descendants running in the background.
    unsafe {
      libc::kill(-(self.0 as i32), libc::SIGKILL);
    }
  }
}

#[cfg(windows)]
pub(super) struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ProcessTree {
  pub(super) fn attach(child: &Child) -> Option<Self> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
    unsafe {
      let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
      if job.is_null() {
        return None;
      }
      let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
      limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
      if SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        &limits as *const _ as _,
        std::mem::size_of_val(&limits) as u32,
      ) == 0
        || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
      {
        CloseHandle(job);
        return None;
      }
      Some(Self(job))
    }
  }
}

#[cfg(windows)]
impl Drop for ProcessTree {
  fn drop(&mut self) {
    unsafe {
      windows_sys::Win32::Foundation::CloseHandle(self.0);
    }
  }
}

#[cfg(not(any(unix, windows)))]
pub(super) struct ProcessTree;
#[cfg(not(any(unix, windows)))]
impl ProcessTree {
  pub(super) fn attach(_: &Child) -> Option<Self> {
    None
  }
}
