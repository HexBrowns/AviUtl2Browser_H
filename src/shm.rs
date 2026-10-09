//! 画素を受け渡す共有メモリ（原作 bridge.dll の FileMappingObject と同じ形）。
//!
//! 先頭 20 バイトがヘッダー（header_size / body_size / version / width / height）、その後ろに BGRA が w*h*4 バイト。
//! 大きさが足りなくなったら別の名前で作り直す。exe は要求に載った名前が変わったら開き直す（protocol.rs）。

use crate::protocol::SHM_HEADER_SIZE;
use anyhow::{Result, bail};
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE,
    UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

/// 最初に確保する本体の大きさ（FHD 1 枚）。これより大きい要求が来たら作り直す
const INITIAL_BODY: usize = 1920 * 1080 * 4;
/// 共有メモリの版（ヘッダーの version。exe は読まない）
const SHM_VERSION: u32 = 3;

static SEQ: AtomicU32 = AtomicU32::new(0);

pub struct SharedImage {
    handle: HANDLE,
    view: *mut u8,
    body_capacity: usize,
    name: String,
}

// 呼び出しは Session の Mutex で直列にしている
unsafe impl Send for SharedImage {}

impl SharedImage {
    fn create(body_capacity: usize) -> Result<Self> {
        let name = format!(
            "Local\\AviUtl2Browser_H.{}.{}",
            unsafe { GetCurrentProcessId() },
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let total = (SHM_HEADER_SIZE + body_capacity) as u64;
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                (total >> 32) as u32,
                total as u32,
                wide.as_ptr(),
            )
        };
        if handle.is_null() {
            bail!("CreateFileMapping failed ({})", unsafe { GetLastError() });
        }
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view.Value.is_null() {
            let e = unsafe { GetLastError() };
            unsafe { CloseHandle(handle) };
            bail!("MapViewOfFile failed ({e})");
        }
        Ok(Self { handle, view: view.Value as *mut u8, body_capacity, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// w×h が入るものを用意し、ヘッダーに大きさを書く。足りなければ作り直す
    pub fn ensure(slot: &mut Option<SharedImage>, w: u32, h: u32) -> Result<&mut SharedImage> {
        let need = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|&n| n > 0 && n <= u32::MAX as usize)
            .ok_or_else(|| anyhow::anyhow!("invalid size {w}x{h}"))?;
        if slot.as_ref().is_none_or(|s| s.body_capacity < need) {
            *slot = None;
            *slot = Some(Self::create(need.max(INITIAL_BODY))?);
        }
        let s = slot.as_mut().unwrap();
        let hdr = [SHM_HEADER_SIZE as u32, s.body_capacity as u32, SHM_VERSION, w, h];
        unsafe {
            std::ptr::copy_nonoverlapping(hdr.as_ptr() as *const u8, s.view, SHM_HEADER_SIZE);
        }
        Ok(s)
    }

    /// 本体の先頭 len バイト
    pub fn body(&self, len: usize) -> &[u8] {
        let len = len.min(self.body_capacity);
        unsafe { std::slice::from_raw_parts(self.view.add(SHM_HEADER_SIZE), len) }
    }
}

impl Drop for SharedImage {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.view as *mut _ });
            CloseHandle(self.handle);
        }
    }
}
