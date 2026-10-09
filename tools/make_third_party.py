"""THIRD_PARTY.md（Rust 以外の同梱物のライセンス表記）を作る。

    python AI/plugins/AviUtl2Browser_H/tools/make_third_party.py

本文は配布物そのものから取る（手で写さない）: CEF の LICENSE.txt、picojson.h と miniz.c の先頭・末尾のコメント。
Chromium の一覧は大きい（約 9MB）ので本文に入れず、同梱する CREDITS.html を指す。Rust の依存は THIRD_PARTY_CRATES.md。
"""
import io
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
EXE = HERE / "exe"


def cef_root():
    text = (EXE / "CMakeLists.txt").read_text(encoding="utf-8")
    m = re.search(r'set\(CEF_DIST_NAME "([^"]+)"\)', text)
    assert m, "CEF_DIST_NAME が無い"
    return EXE / "third_party" / m.group(1), m.group(1)


def comment_block(text, start_marker, end_marker):
    i = text.index(start_marker)
    j = text.index(end_marker, i)
    return text[i:j].strip()


def main():
    root, dist = cef_root()
    cef_license = (root / "LICENSE.txt").read_text(encoding="utf-8").strip()
    pico = (EXE / "browser" / "picojson.h").read_text(encoding="utf-8")
    pico_license = comment_block(pico, "Copyright 2009-2010", "*/")
    pico_license = "\n".join(line.lstrip(" *") for line in pico_license.splitlines()).strip()
    miniz = (EXE / "browser" / "miniz.c").read_text(encoding="utf-8", errors="replace")
    miniz_license = comment_block(miniz, "This is free and unencumbered software", "*/")
    miniz_license = "\n".join(line.strip() for line in miniz_license.splitlines()).strip()

    body = f"""# 同梱する Rust 以外のもののライセンス

`python AI/plugins/AviUtl2Browser_H/tools/make_third_party.py` が配布物から作ったもの（手で直さない）。
Rust の依存は `THIRD_PARTY_CRATES.md`。

| もの | 用途 | ライセンス |
|---|---|---|
| aviutl_browser（oov） | 描画プロセスと互換モジュールの元 | MIT（`LICENSE`） |
| Chromium Embedded Framework（{dist}） | 描画 | BSD-3-Clause（下） |
| Chromium と、その依存 | 描画 | 一覧は同梱の `Script/browser/exe/CREDITS.html` |
| PicoJSON | exe の JSON | BSD-2-Clause（下） |
| miniz 1.16 beta r1 | `.abc`（zip）の読み込み | Unlicense（パブリックドメイン、下） |

## Chromium Embedded Framework

```
{cef_license}
```

## PicoJSON

```
{pico_license}
```

## miniz

```
{miniz_license}
```
"""
    out = HERE / "THIRD_PARTY.md"
    io.open(out, "w", encoding="utf-8", newline="\n").write(body)
    o = io.open(1, "w", encoding="utf-8", closefd=False)
    o.write("[INFO] %s を書いた\n" % out)
    o.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
