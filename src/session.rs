//! 1 回の描画（原作の `bridge.call(exe, data, "w")` に当たる）。
//!
//! exe が動いていなければ起動し、共有メモリに大きさを書き、要求を送って応答を待つ。
//! 時間切れや通信の失敗では exe を落とし、次の呼び出しで起動し直す。
//! ただし起動・通信に失敗した直後は、しばらく起動し直さない（`RestartBackoff`）。

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

/// 起動・通信に失敗してから、次に起動するまで待つ時間（最初）。続けて失敗するたびに倍にする
pub const RESTART_DELAY_MIN: Duration = Duration::from_secs(2);
/// 待つ時間の上限
pub const RESTART_DELAY_MAX: Duration = Duration::from_secs(30);

/// exe の起動・通信に失敗した後、次の起動まで時間を空ける。
///
/// exe が起動直後に落ちる状態（実行時ファイルが欠けている・CEF の初期化に失敗する等）だと、
/// 呼び出しのたび（毎フレーム）に CEF を起動し直して重くなるため。
/// 待っている間は、前の失敗と**同じ文字列**を返す（`browser.lua` の `warn_once` は文字列ごとに 1 回だけ
/// ログに出すので、ログは最初の失敗の 1 行だけになる）。応答を 1 回受け取れたら元に戻す。
#[derive(Debug, Default)]
pub struct RestartBackoff {
    failures: u32,
    retry_at: Option<Instant>,
    last_error: String,
}

impl RestartBackoff {
    /// 続けて `failures` 回失敗した後に待つ時間
    pub fn delay(failures: u32) -> Duration {
        if failures == 0 {
            return Duration::ZERO;
        }
        let shift = (failures - 1).min(16);
        RESTART_DELAY_MIN.saturating_mul(1u32 << shift).min(RESTART_DELAY_MAX)
    }

    /// 待っている間なら、前の失敗の理由を返す
    pub fn blocked(&self, now: Instant) -> Option<&str> {
        match self.retry_at {
            Some(t) if now < t => Some(&self.last_error),
            _ => None,
        }
    }

    /// 失敗を記録し、次に起動してよい時刻までの待ち時間を返す
    pub fn failed(&mut self, now: Instant, error: &str) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let d = Self::delay(self.failures);
        self.retry_at = Some(now + d);
        self.last_error = error.to_string();
        d
    }

    /// 応答を受け取れた
    pub fn succeeded(&mut self) {
        self.failures = 0;
        self.retry_at = None;
        self.last_error.clear();
    }
}

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
    backoff: RestartBackoff,
}

impl Session {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe, child: None, shm: None, backoff: RestartBackoff::default() }
    }

    pub fn render(&mut self, p: &Params) -> Result<Rendered<'_>> {
        let now = Instant::now();
        let deadline = now + CALL_TIMEOUT;
        if self.child.as_ref().is_some_and(|c| !c.is_alive()) {
            self.child = None;
        }
        if self.child.is_none() {
            if let Some(reason) = self.backoff.blocked(now) {
                bail!("{reason}");
            }
            match Child::spawn(&self.exe) {
                Ok(c) => self.child = Some(c),
                Err(e) => {
                    let msg = format!("{e:#}");
                    self.backoff.failed(now, &msg);
                    bail!("{msg}");
                }
            }
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
                let msg = match e {
                    IoError::Timeout => format!("timeout ({} s)", CALL_TIMEOUT.as_secs()),
                    IoError::Broken(s) => format!("aviutl_browser.exe: {s}"),
                };
                // 起動直後に落ちる exe を毎フレーム起動し直さない
                self.backoff.failed(Instant::now(), &msg);
                bail!("{msg}");
            }
        };
        self.backoff.succeeded();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_doubles_up_to_max() {
        let d: Vec<u64> = (0..8).map(|n| RestartBackoff::delay(n).as_secs()).collect();
        assert_eq!(d, vec![0, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(RestartBackoff::delay(u32::MAX), RESTART_DELAY_MAX);
    }

    #[test]
    fn backoff_blocks_until_retry_time_and_keeps_message() {
        let t0 = Instant::now();
        let mut b = RestartBackoff::default();
        assert_eq!(b.blocked(t0), None);
        assert_eq!(b.failed(t0, "boom"), Duration::from_secs(2));
        assert_eq!(b.blocked(t0), Some("boom"));
        assert_eq!(b.blocked(t0 + Duration::from_millis(1999)), Some("boom"));
        assert_eq!(b.blocked(t0 + Duration::from_secs(2)), None);
        // 続けて失敗すると延びる
        let t1 = t0 + Duration::from_secs(2);
        assert_eq!(b.failed(t1, "boom"), Duration::from_secs(4));
        assert_eq!(b.blocked(t1 + Duration::from_secs(3)), Some("boom"));
        // 応答を受け取れたら元に戻る
        b.succeeded();
        assert_eq!(b.blocked(t1), None);
        assert_eq!(b.failed(t1, "again"), Duration::from_secs(2));
    }

    #[test]
    fn missing_exe_is_not_retried_every_call() {
        let dir = std::env::temp_dir().join("aviutl2_browser_h_test_missing");
        let mut s = Session::new(dir.join("aviutl_browser.exe"));
        let p = Params {
            path: "x.abc",
            use_dir: false,
            dev: false,
            userfile: "",
            tabid: "",
            param: "",
            width: 4,
            height: 4,
        };
        let first = format!("{:#}", s.render(&p).err().expect("must fail"));
        assert!(s.backoff.blocked(Instant::now()).is_some(), "waiting after the failure");
        // 待っている間は同じ文字列（browser.lua の warn_once でログが 1 行で済む）
        let second = format!("{:#}", s.render(&p).err().expect("must fail"));
        assert_eq!(first, second);
        assert_eq!(s.backoff.failures, 1, "second call must not try to spawn again");
    }

    /// 起動はできるがすぐ終わる exe（応答を返さない）。hostname.exe は標準入力を読まずに 1 行書いて終わる
    #[test]
    fn exe_that_exits_at_once_is_not_restarted_every_call() {
        let windir = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = std::path::Path::new(&windir).join("System32").join("hostname.exe");
        if !exe.is_file() {
            return;
        }
        let mut s = Session::new(exe);
        let p = Params {
            path: "x.abc",
            use_dir: false,
            dev: false,
            userfile: "",
            tabid: "",
            param: "",
            width: 4,
            height: 4,
        };
        let first = format!("{:#}", s.render(&p).err().expect("must fail"));
        assert!(first.starts_with("aviutl_browser.exe:"), "{first}");
        assert!(s.child.is_none());
        let second = format!("{:#}", s.render(&p).err().expect("must fail"));
        assert_eq!(first, second);
        assert_eq!(s.backoff.failures, 1, "second call must not spawn again");
    }
}
