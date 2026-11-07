//! Windows pipe security (SEC-02): an owner-only DACL and same-user peer checks.
//!
//! This module is listed in `tools/unsafe-allowlist.txt`. Every `unsafe` block calls a Win32
//! function with pointers that live for the whole call, and frees what Windows allocates.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Read a NUL-terminated wide string that Windows allocated with LocalAlloc, then free it.
fn take_local_wide(pointer: *mut u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0;
    // SAFETY: Windows returned a valid NUL-terminated string; we stop at the terminator.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` elements were just read from the same allocation.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
    // SAFETY: the pointer came from a Win32 call documented to allocate with LocalAlloc.
    unsafe { LocalFree(pointer as *mut c_void) };
    text
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle was opened by this module and is closed exactly once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn token_user_sid(process: HANDLE) -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is a valid process handle; `token` receives a new handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error());
    }
    let token = OwnedHandle(token);
    let mut needed = 0_u32;
    // SAFETY: a null buffer with length 0 asks only for the required size.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 || needed > 4096 {
        return Err(last_error());
    }
    // u64 storage keeps the TOKEN_USER structure suitably aligned.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `needed` bytes and lives for the call.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr() as *mut c_void,
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER that points into the buffer.
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: the SID pointer is valid while `buffer` lives; `text` receives a LocalAlloc string.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(last_error());
    }
    Ok(take_local_wide(text))
}

/// The current user's SID, for example `S-1-5-21-…`.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    token_user_sid(unsafe { GetCurrentProcess() })
}

/// The user SID that owns another process.
pub fn process_user_sid(pid: u32) -> io::Result<String> {
    // SAFETY: OpenProcess has no pointer arguments; a null result is checked.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(last_error());
    }
    let process = OwnedHandle(process);
    token_user_sid(process.0)
}

/// Security attributes whose DACL allows only the current user (protected, so nothing is
/// inherited, and no other account, not even administrators, gets an entry).
pub struct OwnerOnly {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl OwnerOnly {
    pub fn new() -> io::Result<Self> {
        let sddl = wide(&format!("D:P(A;;GA;;;{})", current_user_sid()?));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated and outlives the call; Windows allocates the descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    /// Pointer for APIs that take `LPSECURITY_ATTRIBUTES`. Valid while `self` lives.
    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        &mut self.attributes as *mut SECURITY_ATTRIBUTES as *mut c_void
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.descriptor) };
    }
}

// SAFETY: the descriptor is an immutable heap allocation owned by this value.
unsafe impl Send for OwnerOnly {}

/// Create one instance of a private pipe: owner-only DACL, remote clients rejected.
/// `first` makes creation fail if any process already owns the name.
pub fn create_pipe(
    name: &str,
    security: &mut OwnerOnly,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .in_buffer_size(64 * 1024)
        .out_buffer_size(64 * 1024);
    // SAFETY: the attributes pointer comes from `OwnerOnly`, which outlives this call; Windows
    // copies the descriptor into the new pipe object.
    unsafe { options.create_with_security_attributes_raw(name, security.as_mut_ptr()) }
}

/// The process id of the client on the other end of a server pipe handle.
pub fn pipe_client_pid(handle: BorrowedHandle<'_>) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: a BorrowedHandle is a valid open handle for the duration of the borrow.
    if unsafe { GetNamedPipeClientProcessId(handle.as_raw_handle(), &mut pid) } == 0 {
        return Err(last_error());
    }
    Ok(pid)
}

/// The process id of the server on the other end of a client pipe handle.
pub fn pipe_server_pid(handle: BorrowedHandle<'_>) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: a BorrowedHandle is a valid open handle for the duration of the borrow.
    if unsafe { GetNamedPipeServerProcessId(handle.as_raw_handle(), &mut pid) } == 0 {
        return Err(last_error());
    }
    Ok(pid)
}

/// The DACL of a kernel object in SDDL form, for tests and diagnostics.
pub fn dacl_sddl(handle: BorrowedHandle<'_>) -> io::Result<String> {
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is valid for the borrow; only the descriptor out-pointer is
    // requested, and the others may be null.
    let status = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `descriptor` is valid until freed below; `text` receives a LocalAlloc string.
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    };
    // SAFETY: GetSecurityInfo allocated the descriptor with LocalAlloc.
    unsafe { LocalFree(descriptor) };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(take_local_wide(text))
}
