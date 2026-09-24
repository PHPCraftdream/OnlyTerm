use crate::win::pseudocon::HPCON;
use anyhow::{bail, ensure, Error};
use std::io::Error as IoError;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::um::processthreadsapi::*;
use winapi::um::winnt::HANDLE;

const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x00020016;
const PROC_THREAD_ATTRIBUTE_JOB_LIST: usize = 0x0002000D;

pub struct ProcThreadAttributeList {
    data: Vec<usize>,
    job_handle: Option<Box<HANDLE>>,
}

impl ProcThreadAttributeList {
    pub fn with_capacity(num_attributes: DWORD) -> Result<Self, Error> {
        let mut bytes_required: usize = 0;
        // SAFETY: Per MSDN, calling with a NULL list pointer is the documented
        // way to query the required buffer size; it always fails with
        // ERROR_INSUFFICIENT_BUFFER and writes the size to `bytes_required`.
        unsafe {
            InitializeProcThreadAttributeList(
                ptr::null_mut(),
                num_attributes,
                0,
                &mut bytes_required,
            )
        };
        ensure!(
            bytes_required > 0,
            "empty process attribute list allocation"
        );
        // The opaque WinAPI list has pointer alignment.
        let mut data = vec![0usize; bytes_required.div_ceil(mem::size_of::<usize>())];

        let attr_ptr = data.as_mut_ptr() as *mut _;
        // SAFETY: `attr_ptr` is aligned, zeroed storage of at least
        // `bytes_required` bytes; the attribute count matches the first call.
        let res = unsafe {
            InitializeProcThreadAttributeList(attr_ptr, num_attributes, 0, &mut bytes_required)
        };
        ensure!(
            res != 0,
            "InitializeProcThreadAttributeList failed: {}",
            IoError::last_os_error()
        );
        Ok(Self {
            data,
            job_handle: None,
        })
    }

    pub fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.data.as_mut_ptr() as *mut _
    }

    pub fn set_pty(&mut self, con: HPCON) -> Result<(), Error> {
        // SAFETY: `self.as_mut_ptr()` returns a valid, initialized attribute
        // list from `with_capacity`. `con` is a valid HPCON from
        // CreatePseudoConsole. The attribute size is `size_of::<HPCON>()`.
        let res = unsafe {
            UpdateProcThreadAttribute(
                self.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                con,
                mem::size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        ensure!(
            res != 0,
            "UpdateProcThreadAttribute failed: {}",
            IoError::last_os_error()
        );
        Ok(())
    }

    pub fn set_job(&mut self, job: HANDLE) -> Result<(), Error> {
        ensure!(!job.is_null(), "job handle is null");
        ensure!(self.job_handle.is_none(), "job attribute already set");

        let attr_ptr = self.as_mut_ptr();
        let mut handle = Box::new(job);
        // SAFETY: The list is initialized and `handle` stays boxed until
        // `DeleteProcThreadAttributeList` runs, as required for lpValue.
        let res = unsafe {
            UpdateProcThreadAttribute(
                attr_ptr,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                &mut *handle as *mut HANDLE as *mut _,
                mem::size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        let error = (res == 0).then(IoError::last_os_error);
        self.job_handle = Some(handle);
        if let Some(error) = error {
            bail!("UpdateProcThreadAttribute(JOB_LIST) failed: {error}");
        }
        Ok(())
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        // SAFETY: The list was successfully initialized in `with_capacity`
        // and `data` still holds the backing buffer.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}
