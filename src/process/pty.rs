//! Windows ConPTY (Pseudo Console) wrapper.
//!
//! Provides a virtual console for child processes so that grandchild processes
//! (e.g. Python multiprocessing workers) receive valid console handles even when
//! they are created with `bInheritHandles=FALSE`.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Write};
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, InitializeProcThreadAttributeList,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTUPINFOEXW,
};

const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x00020016;

/// Wraps a Windows ConPTY session and the process spawned inside it.
pub struct PtyProcess {
    console: HPCON,
    process_handle: HANDLE,
    thread_handle: HANDLE,
    pid: u32,
    output: Option<File>,
    input: File,
    _attr_list_buf: Vec<u8>,
}

// SAFETY: The HANDLEs and HPCON are not aliased and are only used from one
// thread at a time. PtyProcess is moved into a spawn_blocking closure.
unsafe impl Send for PtyProcess {}

impl PtyProcess {
    /// Spawn a command inside a new ConPTY session.
    ///
    /// `command` is passed to `cmd.exe /C` for shell expansion.
    pub fn spawn(
        command: &str,
        working_dir: Option<&Path>,
        env: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<Self> {
        unsafe { Self::spawn_inner(command, working_dir, env) }
    }

    unsafe fn spawn_inner(
        command: &str,
        working_dir: Option<&Path>,
        env: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<Self> {
        // --- Create two pipe pairs for ConPTY I/O ---
        // Pipe 1: PTY input  (lnch writes → ConPTY reads)
        let (pty_input_read, pty_input_write) = create_pipe()?;
        // Pipe 2: PTY output (ConPTY writes → lnch reads)
        let (pty_output_read, pty_output_write) = create_pipe()?;

        // --- Create pseudo console ---
        let size = COORD { X: 200, Y: 50 };
        let mut console: HPCON = 0;
        let hr = CreatePseudoConsole(size, pty_input_read, pty_output_write, 0, &mut console);
        if hr != 0 {
            CloseHandle(pty_input_read);
            CloseHandle(pty_input_write);
            CloseHandle(pty_output_read);
            CloseHandle(pty_output_write);
            anyhow::bail!("CreatePseudoConsole failed: HRESULT 0x{:08X}", hr);
        }

        // The PTY-side ends are duplicated into the ConHost; close our copies.
        CloseHandle(pty_input_read);
        CloseHandle(pty_output_write);

        // --- Initialize startup info with ConPTY attribute ---
        let mut attr_list_size: usize = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attr_list_size);
        let mut attr_list_buf = vec![0u8; attr_list_size];
        let attr_list = attr_list_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;

        if InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_list_size) == 0 {
            ClosePseudoConsole(console);
            CloseHandle(pty_input_write);
            CloseHandle(pty_output_read);
            anyhow::bail!(
                "InitializeProcThreadAttributeList failed: {}",
                io::Error::last_os_error()
            );
        }

        if UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            console as *const std::ffi::c_void,
            mem::size_of::<HPCON>(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) == 0
        {
            ClosePseudoConsole(console);
            CloseHandle(pty_input_write);
            CloseHandle(pty_output_read);
            anyhow::bail!(
                "UpdateProcThreadAttribute failed: {}",
                io::Error::last_os_error()
            );
        }

        let mut si_ex: STARTUPINFOEXW = mem::zeroed();
        si_ex.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        si_ex.lpAttributeList = attr_list;

        // --- Build command line: cmd.exe /C <command> ---
        let cmd_line = format!("cmd.exe /C {}", command);
        let mut cmd_wide = to_wide(&cmd_line);

        // --- Build environment block ---
        let env_block = build_environment_block(env);

        // --- Working directory ---
        let wide_cwd: Option<Vec<u16>> = working_dir.map(|p| to_wide(&p.to_string_lossy()));
        let cwd_ptr = wide_cwd
            .as_ref()
            .map_or(ptr::null(), |v| v.as_ptr());

        // --- Create process ---
        let mut proc_info: PROCESS_INFORMATION = mem::zeroed();
        let success = CreateProcessW(
            ptr::null(),
            cmd_wide.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0, // bInheritHandles = FALSE
            EXTENDED_STARTUPINFO_PRESENT | 0x00000400, // CREATE_UNICODE_ENVIRONMENT
            env_block.as_ptr() as *const std::ffi::c_void,
            cwd_ptr,
            &si_ex.StartupInfo,
            &mut proc_info,
        );

        if success == 0 {
            let err = io::Error::last_os_error();
            ClosePseudoConsole(console);
            CloseHandle(pty_input_write);
            CloseHandle(pty_output_read);
            anyhow::bail!("CreateProcessW failed: {}", err);
        }

        let output_file = File::from_raw_handle(pty_output_read as *mut std::ffi::c_void);
        let input_file = File::from_raw_handle(pty_input_write as *mut std::ffi::c_void);

        Ok(PtyProcess {
            console,
            process_handle: proc_info.hProcess,
            thread_handle: proc_info.hThread,
            pid: proc_info.dwProcessId,
            output: Some(output_file),
            input: input_file,
            _attr_list_buf: attr_list_buf,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Take ownership of the output pipe.  Can only be called once.
    pub fn take_output(&mut self) -> Option<File> {
        self.output.take()
    }

    /// Write raw bytes to the ConPTY input (e.g. `\x03` for Ctrl+C).
    #[allow(dead_code)]
    pub fn write_input(&mut self, data: &[u8]) -> io::Result<()> {
        self.input.write_all(data)
    }

    /// Return a duplicate of the input file handle so the caller can send
    /// input (e.g. Ctrl+C) independently of the `PtyProcess` lifetime.
    pub fn write_input_handle(&self) -> io::Result<File> {
        self.input.try_clone()
    }

    /// Block until the process exits.  Returns the exit code.
    pub fn wait(&self) -> Option<i32> {
        unsafe {
            let result = WaitForSingleObject(self.process_handle, 0xFFFFFFFF); // INFINITE
            if result != WAIT_OBJECT_0 {
                return None;
            }
            let mut exit_code: u32 = 0;
            if GetExitCodeProcess(self.process_handle, &mut exit_code) == 0 {
                return None;
            }
            Some(exit_code as i32)
        }
    }

    /// Forcefully terminate the process tree.
    #[allow(dead_code)]
    pub fn terminate(&self) {
        unsafe {
            TerminateProcess(self.process_handle, 1);
        }
        run_taskkill(self.pid);
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        unsafe {
            // Close the pseudo console first — this signals EOF on the output pipe
            // and allows the child process tree to be cleaned up by ConHost.
            ClosePseudoConsole(self.console);
            CloseHandle(self.thread_handle);
            CloseHandle(self.process_handle);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

unsafe fn create_pipe() -> anyhow::Result<(HANDLE, HANDLE)> {
    let mut read_handle: HANDLE = INVALID_HANDLE_VALUE;
    let mut write_handle: HANDLE = INVALID_HANDLE_VALUE;
    if CreatePipe(&mut read_handle, &mut write_handle, ptr::null(), 0) == 0 {
        anyhow::bail!("CreatePipe failed: {}", io::Error::last_os_error());
    }
    Ok((read_handle, write_handle))
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Build a Unicode environment block from the current process environment,
/// overlaid with task-specific variables.
fn build_environment_block(extra: Option<&HashMap<String, String>>) -> Vec<u16> {
    let mut env_map: HashMap<String, String> = std::env::vars().collect();

    // Always force unbuffered Python output
    env_map.insert("PYTHONUNBUFFERED".into(), "1".into());

    if let Some(extra) = extra {
        for (k, v) in extra {
            env_map.insert(k.clone(), v.clone());
        }
    }

    // Format: KEY=VALUE\0KEY=VALUE\0\0
    let mut block: Vec<u16> = Vec::new();
    for (key, value) in &env_map {
        let entry = format!("{}={}", key, value);
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    block.push(0); // double-null terminator
    block
}

/// Use taskkill /F /T to kill the entire process tree.
#[allow(dead_code)]
fn run_taskkill(pid: u32) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
