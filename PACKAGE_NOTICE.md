# 導入の前に（AviUtl2Browser_H / ブラウザ表示）

このプラグインは、ブラウザー（Chromium Embedded Framework）を丸ごと同梱しています。
**次のことを了承した上で導入してください。**

- **重い**: 配布ファイルは約 177MB、展開すると約 380MB になります（`Script/browser/exe/`）
- **常駐する**: ブラウザーで描くオブジェクトを最初に描いた時点で `aviutl_browser.exe` と子プロセス数個が起動し、
  AviUtl2 を終了するまで残ります。メモリは合わせて約 370MB 使います
- **止まることがある**: コンテンツの JavaScript が完了を返さないと、その間（最大 30 秒）描画が止まります
- **ネットワークに出られる**: コンテンツは任意のサイトへアクセスできます。毎フレーム外部へアクセスするコンテンツは
  相手に迷惑がかかるので、作らない・使わないでください
- **ウイルス対策ソフトに止められることがある**: `Script/browser/exe/aviutl_browser.exe` を許可してください
- 実行時に `Script/browser/exe/cache/` が作られ、使うほど大きくなります。AviUtl2 を終了していれば消してかまいません
- **無保証**: このプラグインを使用したこと及び使用しなかったことによるいかなる損害についても、作者は責任を負いません
  （原作 aviutl_browser の注意事項を引き継ぎます）

原作: oov 氏の aviutl_browser（https://github.com/oov/aviutl_browser 、MIT）。原作向けのコンテンツ（`.abc` / フォルダ）と、
それを呼ぶスクリプトは `Script/browser/` と `Script/browser/contents/` に置くとそのまま使えます。

