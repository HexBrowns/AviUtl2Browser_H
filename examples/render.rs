//! AviUtl2 なしで exe を動かし、描いた 1 枚を PNG に書く（単体確認と計測用）。
//!
//! cargo run --release --example render -- <exe> <abc|dir> <path> <param> <w> <h> <out.png> [回数]
//!
//! 回数を 2 以上にすると、同じ要求を繰り返して 1 回あたりの時間を出す（2 回目以降の平均）。

use aviutl2_browser_h::session::{Params, Session};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 8 {
        anyhow::bail!("usage: render <exe> <abc|dir> <path> <param> <w> <h> <out.png> [count]");
    }
    let exe = PathBuf::from(&a[1]);
    let use_dir = a[2] == "dir";
    let (w, h): (u32, u32) = (a[5].parse()?, a[6].parse()?);
    let count: usize = a.get(8).map(|s| s.parse()).transpose()?.unwrap_or(1);
    let mut session = Session::new(exe);
    let params = Params {
        path: &a[3],
        use_dir,
        dev: false,
        userfile: "",
        tabid: "",
        param: &a[4],
        width: w,
        height: h,
    };

    let t0 = Instant::now();
    let first = session.render(&params)?;
    let first_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("[INFO] first call: ok={} message={:?} {:.0} ms", first.ok, first.message, first_ms);
    let mut rgba = first.pixels.to_vec();

    if count > 1 {
        let t1 = Instant::now();
        for i in 1..count {
            let p = Params { param: &format!("{}", a[4].replace("{i}", &i.to_string())), ..params };
            let r = session.render(&p)?;
            if !r.ok {
                println!("[WARN] call {i}: ok=false message={:?}", r.message);
            }
            rgba = r.pixels.to_vec();
        }
        let per = t1.elapsed().as_secs_f64() * 1000.0 / (count - 1) as f64;
        println!("[INFO] {w}x{h}: {per:.1} ms per call ({} calls)", count - 1);
    }

    // BGRA → RGBA
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let file = std::fs::File::create(&a[7])?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    let alpha_nonzero = rgba.chunks_exact(4).filter(|p| p[3] != 0).count();
    println!("[INFO] wrote {} (opaque-ish pixels: {alpha_nonzero})", a[7]);

    let t2 = Instant::now();
    session.shutdown();
    println!("[INFO] shutdown {:.0} ms", t2.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}
