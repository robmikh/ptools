use windows::Win32::Foundation::{CloseHandle, HANDLE};

#[repr(transparent)]
pub struct AutoHandle(pub HANDLE);

impl Drop for AutoHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
