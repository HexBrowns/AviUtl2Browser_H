//! aviutl_browser.exe との通信の形式。
//!
//! 原作 oov/aviutl_bridge の形（`[int32 長さ][データ]` を標準入出力で、画素は共有メモリ）を保ち、
//! 要求のヘッダーだけ版 3 にした（文字列を UTF-8 に、共有メモリの名前を毎回載せる）。
//! exe 側の読み手は `exe/browser/aviutl_browser_win.cc` の `ipc_worker`。

use anyhow::{Result, bail};

/// 要求ヘッダーの版。exe 側の `header::version` と揃える
pub const REQUEST_VERSION: u8 = 3;

/// 共有メモリの先頭（原作の bridge.dll と同じ形、20 バイト）
pub const SHM_HEADER_SIZE: usize = 20;

pub const FLAG_USE_DIR: u8 = 1;
pub const FLAG_DEVTOOLS: u8 = 2;

pub struct Request<'a> {
    pub path: &'a str,
    pub use_dir: bool,
    pub dev: bool,
    pub userfile: &'a str,
    pub tabid: &'a str,
    pub fmo_name: &'a str,
    pub param: &'a str,
}

fn len16(name: &str, s: &str) -> Result<u16> {
    match u16::try_from(s.len()) {
        Ok(n) => Ok(n),
        Err(_) => bail!("{name} is too long ({} bytes)", s.len()),
    }
}

/// `[int32 長さ][ヘッダー 10 バイト][path][userfile][tabid][fmo_name][param]` を作る
pub fn encode_request(r: &Request) -> Result<Vec<u8>> {
    let path_len = len16("path", r.path)?;
    let userfile_len = len16("userfile", r.userfile)?;
    let tabid_len = len16("tabid", r.tabid)?;
    let fmo_len = len16("fmo_name", r.fmo_name)?;
    let mut flags = 0u8;
    if r.use_dir {
        flags |= FLAG_USE_DIR;
    }
    if r.dev {
        flags |= FLAG_DEVTOOLS;
    }
    let body_len = 10 + r.path.len() + r.userfile.len() + r.tabid.len() + r.fmo_name.len() + r.param.len();
    let Ok(body_len32) = i32::try_from(body_len) else {
        bail!("request is too large ({body_len} bytes)");
    };
    let mut v = Vec::with_capacity(4 + body_len);
    v.extend_from_slice(&body_len32.to_le_bytes());
    v.push(REQUEST_VERSION);
    v.push(flags);
    v.extend_from_slice(&path_len.to_le_bytes());
    v.extend_from_slice(&userfile_len.to_le_bytes());
    v.extend_from_slice(&tabid_len.to_le_bytes());
    v.extend_from_slice(&fmo_len.to_le_bytes());
    v.extend_from_slice(r.path.as_bytes());
    v.extend_from_slice(r.userfile.as_bytes());
    v.extend_from_slice(r.tabid.as_bytes());
    v.extend_from_slice(r.fmo_name.as_bytes());
    v.extend_from_slice(r.param.as_bytes());
    Ok(v)
}

/// 応答の本体（長さの後ろ）: 1 バイト目が成否、残りが JS の返した文字列（UTF-8）
pub fn decode_response(body: &[u8]) -> Result<(bool, String)> {
    let Some((&ok, rest)) = body.split_first() else {
        bail!("empty response");
    };
    Ok((ok != 0, String::from_utf8_lossy(rest).into_owned()))
}

/// Lua 側（browser.lua）が 16 進で渡した文字列をバイト列に戻す。
/// 旧形式（.obj / .anm）の SJIS や途中の NUL を、本体の文字列変換に通さずに受け取るため
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    fn nib(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    b.chunks_exact(2)
        .map(|p| Some(nib(p[0])? << 4 | nib(p[1])?))
        .collect()
}

/// UTF-8 として正しければそのまま、でなければ CP932（旧形式のスクリプト）として読む
pub fn bytes_to_string(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => cp932_to_string(e.as_bytes()),
    }
}

#[cfg(windows)]
fn cp932_to_string(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(len) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    unsafe {
        let n = MultiByteToWideChar(932, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut w = vec![0u16; n as usize];
        let n = MultiByteToWideChar(932, 0, bytes.as_ptr(), len, w.as_mut_ptr(), n);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        String::from_utf16_lossy(&w[..n as usize])
    }
}

#[cfg(not(windows))]
fn cp932_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_layout() {
        let r = Request {
            path: "絵文字.abc",
            use_dir: false,
            dev: true,
            userfile: "",
            tabid: "t",
            fmo_name: "Local\\x",
            param: "1f423",
        };
        let v = encode_request(&r).unwrap();
        let body_len = i32::from_le_bytes(v[0..4].try_into().unwrap()) as usize;
        assert_eq!(body_len, v.len() - 4);
        assert_eq!(v[4], 3);
        assert_eq!(v[5], FLAG_DEVTOOLS);
        let path_len = u16::from_le_bytes([v[6], v[7]]) as usize;
        assert_eq!(path_len, "絵文字.abc".len());
        assert_eq!(u16::from_le_bytes([v[8], v[9]]), 0);
        assert_eq!(u16::from_le_bytes([v[10], v[11]]), 1);
        assert_eq!(u16::from_le_bytes([v[12], v[13]]) as usize, "Local\\x".len());
        let strings = &v[14..];
        assert_eq!(&strings[..path_len], "絵文字.abc".as_bytes());
        assert!(strings.ends_with(b"Local\\x1f423"));
    }

    #[test]
    fn too_long_path_is_rejected() {
        let long = "a".repeat(70000);
        let r = Request { path: &long, use_dir: true, dev: false, userfile: "", tabid: "", fmo_name: "", param: "" };
        assert!(encode_request(&r).is_err());
    }

    #[test]
    fn response() {
        assert_eq!(decode_response(&[1, b'o', b'k']).unwrap(), (true, "ok".to_string()));
        assert_eq!(decode_response(&[0]).unwrap(), (false, String::new()));
        assert!(decode_response(&[]).is_err());
    }

    #[test]
    fn hex() {
        assert_eq!(decode_hex("").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_hex("00ff41").unwrap(), vec![0, 255, b'A']);
        assert_eq!(decode_hex("0A0b").unwrap(), vec![10, 11]);
        assert!(decode_hex("0").is_none());
        assert!(decode_hex("zz").is_none());
    }

    #[test]
    fn utf8_and_cp932() {
        assert_eq!(bytes_to_string("絵文字".as_bytes().to_vec()), "絵文字");
        // 「絵文字」の CP932
        assert_eq!(bytes_to_string(vec![0x8a, 0x47, 0x95, 0xb6, 0x8e, 0x9a]), "絵文字");
        // CP932 の 2 バイト目が 0x5C（表）
        assert_eq!(bytes_to_string(vec![0x95, 0x5c]), "表");
    }
}
