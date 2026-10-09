//! aviutl_browser.exe の起動・通信・終了。
//!
//! - 標準入出力に名前付きパイプをつなぐ。こちら側は重なり I/O にして、待ち時間に上限を付ける
//!   （原作の bridge.dll は匿名パイプで、exe が応答しないと AviUtl が止まり続けた）
//! - exe はジョブオブジェクトに入れる（KILL_ON_JOB_CLOSE）。本体が落ちても exe と CEF の子プロセスは残らない
//! - スレッドは起こさない（`au2-rs-plugin`「裏のスレッド」）。読み書きは呼び出したスレッドの上で待つ

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::{CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateEventW, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcessId, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    STARTUPINFOW, UpdateProcThreadAttribute, WaitForSingleObject,
};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 終了を頼んでから強制終了するまで待つ時間
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum IoError {
    Timeout,
    Broken(String),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::Timeout => write!(f, "timeout"),
            IoError::Broken(s) => write!(f, "{s}"),
        }
    }
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(h: HANDLE, what: &str) -> Result<Self> {
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            bail!("{what} failed ({})", unsafe { GetLastError() });
        }
        Ok(Self(h))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub struct Child {
    // 落とす順に並べる: 先にパイプを閉じて exe に終わりを知らせる
    stdin: Option<OwnedHandle>,
    stdout: OwnedHandle,
    event: OwnedHandle,
    process: OwnedHandle,
    job: OwnedHandle,
}

unsafe impl Send for Child {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

impl Child {
    pub fn spawn(exe: &Path) -> Result<Self> {
        if !exe.is_file() {
            bail!("{} is not found", exe.display());
        }
        let dir = exe.parent().context("exe has no parent directory")?;

        let job = OwnedHandle::new(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) }, "CreateJobObject")?;
        let mut limit: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limit.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limit as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            bail!("SetInformationJobObject failed ({})", unsafe { GetLastError() });
        }

        let base = format!(
            "\\\\.\\pipe\\AviUtl2Browser_H.{}.{}",
            unsafe { GetCurrentProcessId() },
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let in_name = wide(&format!("{base}.in"));
        let out_name = wide(&format!("{base}.out"));
        let mode = PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
        // こちらが書く側（exe の標準入力）
        let stdin = OwnedHandle::new(
            unsafe {
                CreateNamedPipeW(
                    in_name.as_ptr(),
                    PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    mode,
                    1,
                    1 << 16,
                    1 << 16,
                    0,
                    std::ptr::null(),
                )
            },
            "CreateNamedPipe(in)",
        )?;
        // こちらが読む側（exe の標準出力）
        let stdout = OwnedHandle::new(
            unsafe {
                CreateNamedPipeW(
                    out_name.as_ptr(),
                    PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    mode,
                    1,
                    1 << 16,
                    1 << 16,
                    0,
                    std::ptr::null(),
                )
            },
            "CreateNamedPipe(out)",
        )?;
        let inherit = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        // exe 側の端（同期 I/O）。子へ渡したら閉じる
        let child_in = OwnedHandle::new(
            unsafe {
                CreateFileW(in_name.as_ptr(), GENERIC_READ, 0, &inherit, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut())
            },
            "CreateFile(in)",
        )?;
        let child_out = OwnedHandle::new(
            unsafe {
                CreateFileW(out_name.as_ptr(), GENERIC_WRITE, 0, &inherit, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut())
            },
            "CreateFile(out)",
        )?;
        // 標準エラーは捨てる（CEF のログが標準出力の通信に混ざらないよう、別の行き先にする）
        let nul = wide("NUL");
        let child_err = OwnedHandle::new(
            unsafe {
                CreateFileW(
                    nul.as_ptr(),
                    GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    &inherit,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            },
            "CreateFile(NUL)",
        )?;

        // 子に継がせるハンドルを、この 3 つだけに絞る（本体の他のハンドルを継がせない）
        let handles = [child_in.0, child_out.0, child_err.0];
        let mut attr_size = 0usize;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size) };
        let mut attr_buf = vec![0u8; attr_size];
        let attrs = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut attr_size) } == 0 {
            bail!("InitializeProcThreadAttributeList failed ({})", unsafe { GetLastError() });
        }
        struct AttrGuard(LPPROC_THREAD_ATTRIBUTE_LIST);
        impl Drop for AttrGuard {
            fn drop(&mut self) {
                unsafe { DeleteProcThreadAttributeList(self.0) };
            }
        }
        let _attr_guard = AttrGuard(attrs);
        if unsafe {
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr() as *const _,
                std::mem::size_of_val(&handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            bail!("UpdateProcThreadAttribute failed ({})", unsafe { GetLastError() });
        }

        let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        si.StartupInfo = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: child_in.0,
            hStdOutput: child_out.0,
            hStdError: child_err.0,
            ..unsafe { std::mem::zeroed() }
        };
        si.lpAttributeList = attrs;

        let exe_w = wide_path(exe);
        let mut cmdline = wide(&format!("\"{}\"", exe.display()));
        let dir_w = wide_path(dir);
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            CreateProcessW(
                exe_w.as_ptr(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_SUSPENDED,
                std::ptr::null(),
                dir_w.as_ptr(),
                &si.StartupInfo,
                &mut pi,
            )
        } == 0
        {
            bail!("CreateProcess failed ({})", unsafe { GetLastError() });
        }
        let process = OwnedHandle(pi.hProcess);
        let thread = OwnedHandle(pi.hThread);
        if unsafe { AssignProcessToJobObject(job.0, process.0) } == 0 {
            let e = unsafe { GetLastError() };
            unsafe { TerminateJobObject(job.0, 1) };
            bail!("AssignProcessToJobObject failed ({e})");
        }
        unsafe { ResumeThread(thread.0) };
        drop(thread);
        drop((child_in, child_out, child_err));

        let event = OwnedHandle::new(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) }, "CreateEvent")?;
        Ok(Self { stdin: Some(stdin), stdout, event, process, job })
    }

    pub fn is_alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.0, 0) == WAIT_TIMEOUT }
    }

    /// 重なり I/O を 1 回。deadline を過ぎたら取り消して Timeout
    fn io_once(&self, write: bool, buf: *mut u8, len: u32, deadline: Instant) -> Result<u32, IoError> {
        let handle = if write {
            match &self.stdin {
                Some(h) => h.0,
                None => return Err(IoError::Broken("stdin is closed".into())),
            }
        } else {
            self.stdout.0
        };
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = self.event.0;
        let mut done = 0u32;
        let ok = unsafe {
            if write {
                WriteFile(handle, buf, len, &mut done, &mut ov)
            } else {
                ReadFile(handle, buf, len, &mut done, &mut ov)
            }
        };
        if ok == 0 {
            let e = unsafe { GetLastError() };
            if e != ERROR_IO_PENDING {
                return Err(IoError::Broken(format!("{} failed ({e})", if write { "WriteFile" } else { "ReadFile" })));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = left.as_millis().min(u32::MAX as u128 - 1) as u32;
            let w = unsafe { WaitForSingleObject(self.event.0, ms) };
            if w != WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(handle, &ov);
                    GetOverlappedResult(handle, &ov, &mut done, 1);
                }
                return Err(IoError::Timeout);
            }
        }
        if unsafe { GetOverlappedResult(handle, &ov, &mut done, 0) } == 0 {
            return Err(IoError::Broken(format!("pipe is broken ({})", unsafe { GetLastError() })));
        }
        if done == 0 {
            return Err(IoError::Broken("pipe is closed".into()));
        }
        Ok(done)
    }

    pub fn write_all(&self, mut data: &[u8], deadline: Instant) -> Result<(), IoError> {
        while !data.is_empty() {
            let chunk = data.len().min(1 << 20) as u32;
            let n = self.io_once(true, data.as_ptr() as *mut u8, chunk, deadline)?;
            data = &data[n as usize..];
        }
        Ok(())
    }

    pub fn read_exact(&self, buf: &mut [u8], deadline: Instant) -> Result<(), IoError> {
        let mut pos = 0;
        while pos < buf.len() {
            let chunk = (buf.len() - pos).min(1 << 20) as u32;
            let n = self.io_once(false, buf[pos..].as_mut_ptr(), chunk, deadline)?;
            pos += n as usize;
        }
        Ok(())
    }

    /// 標準入力を閉じて exe に終わりを知らせ、待っても終わらなければジョブごと落とす
    pub fn shutdown(mut self) {
        self.stdin = None;
        let ms = SHUTDOWN_WAIT.as_millis() as u32;
        if unsafe { WaitForSingleObject(self.process.0, ms) } != WAIT_OBJECT_0 {
            unsafe { TerminateJobObject(self.job.0, 1) };
        }
        // CEF の子プロセスが残っていてもジョブを閉じれば消える（KILL_ON_JOB_CLOSE）
    }

    /// すぐに落とす（時間切れで通信が食い違ったとき）
    pub fn kill(self) {
        unsafe { TerminateJobObject(self.job.0, 1) };
    }
}
