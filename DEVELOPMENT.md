# AviUtl2Browser_H（ブラウザ表示）

oov 氏の [aviutl_browser](https://github.com/oov/aviutl_browser)（AviUtl1 用、MIT）の AviUtl2 移植。
HTML / CSS / JavaScript の表示内容をオブジェクトの画像にする。原作向けのコンテンツ（`.abc` / フォルダ、
`AviUtlBrowser.registerRenderer`）と、それを呼ぶスクリプト（`require("browser").execute{...}`）をそのまま使うことを目標にしている。

仕様と進み具合: `AI/specifications/20261009_AviUtl2Browser_H_spec.md`

## 構成

| パス | 中身 | 配置先 |
|---|---|---|
| `src/` | `.mod2`（Rust）。exe の起動・共有メモリ・通信（原作の bridge.dll の役） | `Script/browser/AviUtl2Browser_H.mod2` |
| `exe/browser/` | `aviutl_browser.exe`（C++、CEF）。原作の `src/exe` に `AviUtl2Browser_H:` のコメントの箇所だけ手を入れた | `Script/browser/exe/` |
| `scripts/browser.lua` | 原作と同じ `require("browser")` を出す互換モジュール | `Script/browser/browser.lua`（`require` は呼んだスクリプトのフォルダだけを探す） |
| `scripts/ブラウザ表示.obj2` | 任意のコンテンツを開く汎用オブジェクト | `Script/browser/` |
| `examples/render.rs` | AviUtl2 なしで exe を動かして PNG に書く（単体確認と計測） | 配らない |

## ビルド

1. CEF のバイナリ配布物（minimal）を `exe/third_party/` に展開する（git では追跡しない）。版は `exe/CMakeLists.txt` の `CEF_DIST_NAME`

   ```bash
   curl -L -o cef.tar.bz2 "https://cef-builds.spotifycdn.com/cef_binary_154.0.34%2Bg14c5a08%2Bchromium-154.0.8037.98_windows64_minimal.tar.bz2"
   ```

   SHA-1 は `be54bbaf3222191dec59ffd9c9ec7e8a7f44a26d`。`tar -xjf cef.tar.bz2 -C exe/third_party` で展開する

2. exe（Visual Studio 2022 Build Tools と CMake 3.21 以上）

   ```bash
   cmake -S exe -B exe/build -G "Visual Studio 17 2022" -A x64
   cmake --build exe/build --config Release --target aviutl_browser
   ```

   出力は `exe/build/browser/Release/`。CEF の実行時ファイルも同じ場所へ写される

3. `.mod2`

   ```bash
   cargo build --release
   ```

単体確認（`exe/build/browser/contents/` にコンテンツを置く。exe は自分の `..\contents` を探す）:

```bash
cargo run --release --example render -- exe/build/browser/Release/aviutl_browser.exe dir test "こんにちは" 640 360 out.png
```

## ライセンス

MIT（`LICENSE`。原作の著作権表示を残す）。CEF は BSD-3、Chromium の一覧は CEF 配布物の `CREDITS.html`。
