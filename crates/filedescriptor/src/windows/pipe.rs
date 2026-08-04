use crate::windows::owned_handle::HandleType;
use crate::{Error, FileDescriptor, OwnedHandle, Pipe, Result};
use std::io::Error as IoError;
use std::ptr;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
use winapi::um::namedpipeapi::CreatePipe;
use winapi::um::winnt::HANDLE;

impl Pipe {
    pub fn new() -> Result<Pipe> {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 0,
        };
        let mut read: HANDLE = INVALID_HANDLE_VALUE as _;
        let mut write: HANDLE = INVALID_HANDLE_VALUE as _;
        // SAFETY: `&mut read`/`&mut write` are valid out-pointers for the pipe
        // handles and `&mut sa` is a fully-initialized `SECURITY_ATTRIBUTES`.
        if unsafe { CreatePipe(&mut read, &mut write, &mut sa, 0) } == 0 {
            Err(Error::Pipe(IoError::last_os_error()))
        } else {
            Ok(Pipe {
                read: FileDescriptor {
                    handle: OwnedHandle {
                        handle: read as _,
                        handle_type: HandleType::Pipe,
                    },
                },
                write: FileDescriptor {
                    handle: OwnedHandle {
                        handle: write as _,
                        handle_type: HandleType::Pipe,
                    },
                },
            })
        }
    }
}
