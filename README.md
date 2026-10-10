# AviUtl2Browser_H（ブラウザ表示）マニュアル

**AviUtl2Browser_H**（ブラウザ表示）は、HTML / CSS / JavaScript で描いたものを AviUtl2 のオブジェクトの画像にするプラグインです。
oov 氏の [aviutl_browser](https://github.com/oov/aviutl_browser)（AviUtl1 用）を AviUtl2 へ移植したもので、
**原作向けのコンテンツ（`.abc` / フォルダ）と、それを呼ぶスクリプトをそのまま使えます**（下の「原作向けのコンテンツを使う」の注意を除く）。

- **バージョン:** 0.1.1（2026-10-10: AviUtl2 を 2 つ起動しても両方で描ける・表示用のプログラムが起動できないときに毎フレーム起動し直さない・終了を待ってから閉じる。0.1.0 は 2026-10-09 の初版）
- **作者:** by HexBrowns（原作: oov）
- **必要環境:** AviUtl2 **2.1.11** 以上、Windows 10 / 11（64bit）

---

## 導入の前に（必ず読んでください）

このプラグインは、ブラウザー（Chromium Embedded Framework）を丸ごと同梱しています。**次のことを了承した上で導入してください。**

- **重い**: 配布ファイルは約 177MB、展開すると約 380MB になります
- **常駐する**: ブラウザーで描くオブジェクトを最初に描いた時点で `aviutl_browser.exe` と子プロセス数個が起動し、AviUtl2 を終了するまで残ります。
  メモリは合わせて約 370MB 使います（AviUtl2 を終了すれば消えます）
- **止まることがある**: コンテンツの JavaScript が完了を返さないと、その間（最大 30 秒）描画が止まります
- **ネットワークに出られる**: コンテンツは任意のサイトへアクセスできます。毎フレーム外部へアクセスするコンテンツは相手に迷惑がかかるので、作らない・使わないでください
- **ウイルス対策ソフトに止められることがある**: `Script\browser\exe\aviutl_browser.exe` を許可してください
- 実行時に `Script\browser\exe\cache\` が作られ、使うほど大きくなります。AviUtl2 を 2 つ以上同時に使うと、2 つ目からは `cache_2\` `cache_3\` … を使います。AviUtl2 を終了していれば消してかまいません
- **無保証**: 使用したこと及び使用しなかったことによるいかなる損害についても、作者は責任を負いません（原作の注意事項を引き継ぎます）

## 導入

1. [Releases](https://github.com/HexBrowns/AviUtl2Browser_H/releases) から `HexBrowns.AviUtl2Browser_H-v0.1.1.au2pkg.zip`（約 177MB）をダウンロードし、AviUtl2 のプレビューへドラッグ&ドロップする
   （手で置くなら、中の `Script` フォルダをデータフォルダ `C:\ProgramData\aviutl2` へコピーする）
2. AviUtl2 を再起動する

導入されるのは `Script\browser\` の下だけです。

| パス | 中身 |
|---|---|
| `Script\browser\ブラウザ表示.obj2` | 汎用のカスタムオブジェクト |
| `Script\browser\browser.lua` | 原作と同じ `require("browser")` の入口 |
| `Script\browser\AviUtl2Browser_H.mod2` | AviUtl2 との受け渡し |
| `Script\browser\exe\` | ブラウザー本体（`aviutl_browser.exe` と CEF） |
| `Script\browser\contents\` | コンテンツの置き場 |

**アンインストール**: AviUtl2 を終了してから `Script\browser\` フォルダを消す。

---

## 使い方（ブラウザ表示）

1. オブジェクトの追加の **カスタムオブジェクト** から **HexScript → ブラウザ表示** を選ぶ
2. **コンテンツ** で、`index.html`（そのフォルダが読まれる）か `.abc` ファイルを選ぶ
3. 必要なら **渡す文字列** に、JavaScript へ渡す文字列を書く

最初のフレームはブラウザーの起動で 1 秒ほどかかります。2 フレーム目からは FHD で 1 フレーム 15ms 前後です。

### 設定項目

| 項目 | 範囲 | 初期値 | 説明 |
|---|---|---|---|
| 幅 | 1〜4000 | 1920 | 描く大きさ（ピクセル）。最大画像サイズで頭打ち |
| 高さ | 1〜4000 | 1080 | 同上 |
| コンテンツ | ファイル | なし | `.abc` / `.zip` を選ぶとその zip を、それ以外（`index.html` など）を選ぶとそのファイルのあるフォルダを読む |
| 渡す文字列 | 文字列 | 空 | JavaScript の `params.param` に入る |
| DevTools | オン / オフ | オフ | 開発者ツールの窓を出す |

---

## コンテンツの作り方

原作と同じです。フォルダに `index.html` を置き、JavaScript で描画の関数を登録します。

```html
<!DOCTYPE html>
<meta charset="utf-8">
<p>Hello world <span></span>!</p>
<script>
  // 描画のたびに呼ばれる非同期関数を登録する
  AviUtlBrowser.registerRenderer(async params => {
    // params.param に「渡す文字列」（スクリプトからなら param）が入る
    document.querySelector("span").textContent = params.param;
    return ''; // スクリプトへ返したい文字列があれば返す
  });
</script>
```

- **背景は透明**です。塗らなかったところはオブジェクトでも透明になります
- **画面左上の 1 ピクセルは使えません**（描き終わりの合図に使っています）
- **`html` / `body` に `margin` や `padding` を当てないでください**。合図の位置がずれて、最大 30 秒止まります。余白は `div` に当ててください
- 関数が完了しない（`resolve` しない、例外で止まる）と、最大 30 秒止まります
- `.abc` は、フォルダの**中身**を zip にして拡張子を `.abc` に変えたものです（zip の根に `index.html` が来るようにする）
- ファイル参照で選んだファイルは、HTML から `/userfile` で読めます（スクリプトから `userfile` で渡したとき）
- CSS アニメーションや動画は、AviUtl2 の時間ではなく実時間で進みます。時間に合わせて動かすときは、渡す文字列にフレーム番号などを入れて JavaScript で描き分けてください

---

## スクリプトから呼ぶ

原作と同じ書き方で呼べます。**呼ぶスクリプトは `Script\browser\` に置いてください**（`require("browser")` は呼んだスクリプトと同じフォルダだけを探します）。

```lua
-- 描く大きさの透明な画像を用意する
obj.setoption("drawtarget", "tempbuffer", 1920, 1080)
obj.load("tempbuffer")

local ok, ret = require("browser").execute({
  abc = "mycontent.abc",  -- contents フォルダの .abc（フォルダなら dir = "mycontent"）
  tabid = "",             -- 同じコンテンツを別のタブで開くときの名前（省略可）
  param = "文字列",       -- JavaScript の params.param（省略可）
  userfile = file,        -- ファイル参照で選んだファイル（省略可）
  dev = false,            -- 開発者ツールを出すなら true
})
-- ok: 成功したか、ret: JavaScript が返した文字列（失敗したときは理由）
```

- 画像は呼んだ時点のオブジェクトの大きさで描かれ、オブジェクトの画像と置き換わります
- **同じコンテンツを複数のオブジェクトに置くときは、`tabid` を別々にすると速くなります**（同じタブを共有すると、毎フレーム交互に描き直します）

---

## 原作向けのコンテンツを使う

1. `.abc`（やコンテンツのフォルダ）を `Script\browser\contents\` に置く
2. それを呼ぶスクリプト（`.obj` / `.anm`）を `Script\browser\` に置く
3. AviUtl2 を再起動する

原作のサンプル「絵文字」（`絵文字.obj` と `絵文字.abc`）は、ファイルを変えずにそのまま動きました。

**ただし、テキストの変数を使うもの（原作のサンプル「Markdown」など）はそのままでは動きません。** 次の 2 点を直してください。

- **スクリプトを UTF-8 の新しい形式で保存し直す。** `Markdown.anm` なら、メモ帳などで開いて UTF-8 で `Markdown.anm2` として保存し、元の `Markdown.anm` は消す
  （同じ名前のスクリプトは 2 つ置けません）。AviUtl2 では、古い形式（`.anm` / `.obj`）のスクリプトから、テキストのスクリプト制御（`<?markdown=[==[ … ]==]?>`）で作った変数が見えません
- **テキストには見える文字を 1 つ以上入れる。** `<?markdown=[==[ … ]==]?>` だけだとテキストの画像が無く、後ろの効果（Markdown）が実行されません。末尾に `.` などを 1 文字置いてください（その文字は Markdown の表示で置き換わります）

---

## 原作との違い

- 文字列は UTF-8 で受け渡します（原作は Shift_JIS）。古い形式のスクリプトから渡した文字列も、AviUtl2 が UTF-8 にしてから渡します
- **`tabid` を変えると、本当に別のタブになります**（原作では別のタブにならない不具合がありました）
- **JavaScript が失敗（reject）したとき、`ok` が `false` になります**（原作は常に `true` でした）
- JavaScript が 30 秒以内に完了しないと、`false, "timeout"` を返してその時点の絵を出します（原作はそのまま止まり続けることがありました）
- ブラウザーの部品（CEF）を新しい版にしました。サンドボックスは使っていません

## 注意事項・既知の制限

- 開発者ツール（DevTools）の窓は直接閉じられません（原作と同じ。**DevTools** をオフにすると閉じます）。原作では、この窓で F5 を押すと HTML を読み直せました（この移植ではまだ確かめていません）
- 「キャッシュを破棄」で、コンテンツの HTML が読み直されるかは確かめていません。HTML を直して反映されないときは AviUtl2 を再起動してください
- ブラウザーの言語ファイルは英語と日本語だけを同梱しています（ブラウザーの右クリックメニューなどの言葉だけで、表示には影響しません）
