//! AviUtl2Browser_H.mod2 — aviutl_browser（oov, MIT）の AviUtl2 移植の受け口。
//!
//! Lua からは `Script/browser/browser.lua`（原作と同じ `require("browser").execute{...}`）経由で呼ぶ。
//! 原作の bridge.dll が持っていた役（exe の起動・共有メモリ・標準入出力の通信）をここで持つ。
//! 仕様: AI/specifications/20261009_AviUtl2Browser_H_spec.md

pub mod process;
pub mod protocol;
pub mod session;
pub mod shm;

use aviutl2::{
    AnyResult,
    module::{ScriptModuleCallHandle, ScriptModuleFunctions},
};
use session::{Params, Session};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::ThreadId;

#[aviutl2::plugin(ScriptModule)]
struct AviUtl2BrowserModule {
    session: Mutex<Session>,
    /// 返した画素の写し。呼び出したスレッドごとに持つ（Lua が putpixeldata するまで中身を変えないため）
    images: Mutex<HashMap<ThreadId, Vec<u8>>>,
}

/// この DLL（.mod2）のあるフォルダ
fn module_dir() -> Option<PathBuf> {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
        GetModuleHandleExW,
    };
    unsafe {
        let mut module = std::ptr::null_mut();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            module_dir as *const u16,
            &mut module,
        ) == 0
        {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
        if n == 0 || n >= buf.len() {
            return None;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
        path.parent().map(|p| p.to_path_buf())
    }
}

/// exe の場所: `Script/browser/exe/aviutl_browser.exe`（原作と同じ並び）
pub fn default_exe_path(dir: &std::path::Path) -> PathBuf {
    dir.join("exe").join("aviutl_browser.exe")
}

impl aviutl2::module::ScriptModule for AviUtl2BrowserModule {
    fn new(_info: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        let dir = module_dir().unwrap_or_default();
        Ok(Self { session: Mutex::new(Session::new(default_exe_path(&dir))), images: Mutex::new(HashMap::new()) })
    }

    fn plugin_info(&self) -> aviutl2::module::ScriptModuleTable {
        aviutl2::module::ScriptModuleTable {
            information: format!("AviUtl2Browser_H v{} by HexBrowns (port of aviutl_browser by oov)", env!("CARGO_PKG_VERSION")),
            functions: Self::functions(),
        }
    }
}

impl Drop for AviUtl2BrowserModule {
    fn drop(&mut self) {
        // 本体が DLL を外す前に exe を終わらせる（スレッドは持っていない）
        if let Ok(mut s) = self.session.lock() {
            s.shutdown();
        }
    }
}

/// 16 進で渡された文字列を読む（browser.lua の hex）。UTF-8 でなければ CP932 として読む
fn hex_param(params: &ScriptModuleCallHandle, index: usize) -> Result<String, String> {
    let s = params.get_param_str(index).unwrap_or_default();
    match protocol::decode_hex(&s) {
        Some(b) => Ok(protocol::bytes_to_string(b)),
        None => Err(format!("argument #{} is not a hex string", index + 1)),
    }
}

fn bool_param(params: &ScriptModuleCallHandle, index: usize) -> bool {
    params.get_param_boolean(index).unwrap_or(false)
}

impl AviUtl2BrowserModule {
    fn execute_inner(&self, params: &ScriptModuleCallHandle) -> Result<(Option<*const u8>, bool, String), String> {
        let path = hex_param(params, 0)?;
        let use_dir = bool_param(params, 1);
        let dev = bool_param(params, 2);
        let userfile = hex_param(params, 3)?;
        let tabid = hex_param(params, 4)?;
        let param = hex_param(params, 5)?;
        let w = params.get_param_int(6).unwrap_or(0);
        let h = params.get_param_int(7).unwrap_or(0);
        if w <= 0 || h <= 0 {
            return Err(format!("invalid size {w}x{h}"));
        }
        let mut session = self.session.lock().map_err(|e| e.to_string())?;
        let r = session
            .render(&Params {
                path: &path,
                use_dir,
                dev,
                userfile: &userfile,
                tabid: &tabid,
                param: &param,
                width: w as u32,
                height: h as u32,
            })
            .map_err(|e| format!("{e:#}"))?;
        let mut images = self.images.lock().map_err(|e| e.to_string())?;
        let buf = images.entry(std::thread::current().id()).or_default();
        buf.clear();
        buf.extend_from_slice(r.pixels);
        Ok((Some(buf.as_ptr()), r.ok, r.message))
    }
}

#[aviutl2::module::functions]
impl AviUtl2BrowserModule {
    /// execute(path_hex, use_dir, dev, userfile_hex, tabid_hex, param_hex, w, h) -> data|nil, ok, message
    ///
    /// data は BGRA（ストレート）w*h*4 バイト。`obj.putpixeldata("object", data, w, h, "bgra")` で書く。
    /// 失敗したとき（exe が無い・時間切れなど）は nil, false, 理由
    #[direct]
    fn execute(&self, params: &mut ScriptModuleCallHandle) {
        match self.execute_inner(params) {
            Ok((ptr, ok, message)) => {
                let _ = params.push_result((ptr, ok, message));
            }
            Err(message) => {
                let _ = params.push_result((None::<*const u8>, false, message));
            }
        }
    }

    /// version() -> 版の文字列
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

aviutl2::register_script_module!(AviUtl2BrowserModule);
