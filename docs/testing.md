# ホストでの動作確認

Linux 上のプラグインホストを使って、プラグインの読み込み、画面の表示、音のパススルーを確認する方法を説明します。

[ドキュメント一覧に戻る](README.md) ・ [README](../README.md)

## 方針

普段の確認は Linux 上で行い、Windows 固有の部分だけを区切りのよいところで Windows 上で確認します。

| 環境 | 確認すること |
|---|---|
| Linux（日常の確認） | 読み込み、エディターの表示、パススルー、オートメーション、プロジェクトの保存と復元、検証ツール |
| Windows（リリース前など） | Visual C++ ランタイムへの依存、WASAPI、Windows での OpenGL 描画、インストーラー |

## 自動テスト

`tools/host-tests/run.sh` は、次の 4 つを順に実行します。

| 手順 | 内容 |
|---|---|
| bundle | リリースビルドで VST3 / CLAP を作成し、`~/.vst3` と `~/.clap` にリンクします |
| clap-validator | CLAP 版を検証します。既知の失敗（[#3](https://github.com/My-MC/shirabe/issues/3)）は除外しています |
| pluginval | VST3 版を strictness 5 で検証します。`DISPLAY` があればエディターのテストも行います |
| Ardour | ヘッドレスの Ardour で、テスト信号をプラグインに通した書き出しと、プラグインを無効にした書き出しを比較します |

Ardour の比較では、2 つの書き出しが 32 ビット浮動小数点でサンプル単位まで一致することを確認します。

あわせて、テストオシレーターを有効にした書き出しが一致しないことも確認し、比較そのものが機能していることを確かめます。

### 必要なもの

| ツール | 入手方法 |
|---|---|
| Ardour 8 | Debian 13 では `sudo apt-get install ardour` |
| clap-validator | [GitHub のリリース](https://github.com/free-audio/clap-validator/releases) |
| pluginval | [GitHub のリリース](https://github.com/Tracktion/pluginval/releases) |

### 実行

次のコマンドで、すべての確認を実行します。

```sh
tools/host-tests/run.sh
```

すでにビルド済みのバンドルを使うときは、次のように実行します。

```sh
SKIP_BUNDLE=1 tools/host-tests/run.sh
```

使える環境変数は次のとおりです。

| 変数 | 内容 |
|---|---|
| `SKIP_BUNDLE` | `1` にすると、ビルドを省略します |
| `PLUGINVAL` | pluginval の場所（既定は `PATH` 上の `pluginval`） |
| `CLAP_VALIDATOR` | clap-validator の場所（既定は `PATH` 上の `clap-validator`） |
| `SHIRABE_GL_SHIM` | エディターのテストのときに `LD_LIBRARY_PATH` の先頭に追加するディレクトリ |

Ardour の作業ファイルは `target/host-tests/` に作られ、実行のたびに作り直されます。

## GPU のない環境での注意

エディターは OpenGL 3.2 と sRGB 対応のフレームバッファを使います。

Xvfb や VNC の仮想画面と Mesa（llvmpipe）の組み合わせでは sRGB 対応のフレームバッファが提供されないため、エディターを開けません。

その場合は、テスト用に sRGB の要求だけを取り除く `libGL` のラッパーを用意し、`SHIRABE_GL_SHIM` で指定します。

このラッパーはテスト専用で、配布物には含めません。

OpenGL を使えない環境でエディターを開いたときにホストごと終了する問題は [#1](https://github.com/My-MC/shirabe/issues/1) で扱っています。

## 手動での確認（Ardour）

自動テストで作られたセッション `target/host-tests/session/pt.ardour` を Ardour で開くと、プラグインを挿したバスがそのまま使えます。

1. Ardour を起動し、セッションを開きます。音声デバイスがない場合は、ALSA の `snd-dummy` モジュールを読み込み、Audio System を ALSA、デバイスを Dummy にします。
2. ミキサー画面で `Bus 1` の `Shirabe Meter Suite` をダブルクリックし、エディターを開きます。
3. 再生すると、テスト信号のピーク（L 約 -4.4 dBFS、R 約 -9.1 dBFS）が表示されます。
