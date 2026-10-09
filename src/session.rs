//! 1 回の描画（原作の `bridge.call(exe, data, "w")` に当たる）。
//!
//! exe が動いていなければ起動し、共有メモリに大きさを書き、要求を送って応答を待つ。
//! 時間切れや通信の失敗では exe を落とし、次の呼び出しで起動し直す。

use crate::process::{Child, IoError};
use crate::protocol::{self, Request};
use crate::shm::SharedImage;
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 1 回の呼び出しで待つ上限。exe 側は JS の完了を 30 秒待つ（原作と同じ）ので、それより少し長くする
pub const CALL_TIMEOUT: Duration = Duration::from_secs(35);

/// 応答の上限（JS が返す文字列）。壊れた長さを読んで巨大な確保をしないため
const MAX_RESPONSE: usize = 64 << 20;

pub struct Params<'a> {
    pub path: &'a str,
    pub use_dir: bool,
    pub dev: bool,
    pub userfile: &'a str,
    pub tabid: &'a str,
    pub param: &'a str,
    pub width: u32,
    pub height: u32,
}

pub struct Rendered<'a> {
    pub ok: bool,
    pub message: String,
    /// BGRA（ストレートアルファ）、上の行から。width*height*4 バイト
    pub pixels: &'a [u8],
}

pub struct Session {
    exe: PathBuf,
    child: Option<Child>,
    shm: Option<SharedImage>,
}

impl Session {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe, child: None, shm: None }
    }

    pub fn render(&mut self, p: &Params) -> Result<Rendered<'_>> {
        let deadline = Instant::now() + CALL_TIMEOUT;
        if self.child.as_ref().is_some_and(|c| !c.is_alive()) {
            self.child = None;
        }
        if self.child.is_none() {
            self.child = Some(Child::spawn(&self.exe)?);
        }
        let shm = SharedImage::ensure(&mut self.shm, p.width, p.height)?;
        let req = protocol::encode_request(&Request {
            path: p.path,
            use_dir: p.use_dir,
            dev: p.dev,
            userfile: p.userfile,
            tabid: p.tabid,
            fmo_name: shm.name(),
            param: p.param,
        })?;
        let child = self.child.as_ref().unwrap();
        let result = (|| -> Result<Vec<u8>, IoError> {
            child.write_all(&req, deadline)?;
            let mut len = [0u8; 4];
            child.read_exact(&mut len, deadline)?;
            let len = i32::from_le_bytes(len);
            if len < 1 || len as usize > MAX_RESPONSE {
                return Err(IoError::Broken(format!("invalid response length {len}")));
            }
            let mut body = vec![0u8; len as usize];
            child.read_exact(&mut body, deadline)?;
            Ok(body)
        })();
        let body = match result {
            Ok(b) => b,
            Err(e) => {
                // 応答が遅れて届くと次の要求と食い違うので、exe を落として次で起動し直す
                if let Some(c) = self.child.take() {
                    c.kill();
                }
                match e {
                    IoError::Timeout => bail!("timeout ({} s)", CALL_TIMEOUT.as_secs()),
                    IoError::Broken(s) => bail!("aviutl_browser.exe: {s}"),
                }
            }
        };
        let (ok, message) = protocol::decode_response(&body)?;
        let shm = self.shm.as_ref().unwrap();
        let pixels = shm.body(p.width as usize * p.height as usize * 4);
        Ok(Rendered { ok, message, pixels })
    }

    /// exe を終わらせる（プラグインが外れるとき）
    pub fn shutdown(&mut self) {
        if let Some(c) = self.child.take() {
            c.shutdown();
        }
        self.shm = None;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}
