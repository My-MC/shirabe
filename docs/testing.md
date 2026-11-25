# ホストでの動作確認

Linux 上のプラグインホストを使って、プラグインの読み込み、画面の表示、音のパススルーを確認する方法を説明します。

[ドキュメント一覧に戻る](README.md) ・ [README](../README.md)

## 方針

普段の確認は Linux 上で行い、Windows 固有の部分だけを区切りのよいところで Windows 上で確認します。

| 環境 | 確認すること |
|---|---|
| Linux（日常の確認） | 読み込み、エディターの表示、パススルー、オートメーション、プロジェクトの保存と復元、検証ツール |
| Windows（リリース前など） | Visual C++ ランタイムへの依存、WASAPI、Windows での OpenGL 描画、インストーラー |

## 計測処理の単体テスト

`cargo test --workspace` では、計測処理を次の信号で検証します。

| 対象 | 内容 | 許容範囲 |
|---|---|---|
| ラウドネス | EBU Tech 3341 の合成信号（ケース 1〜6、9）を 44.1 / 48 / 88.2 / 96 kHz で再現 | ±0.1 LU |
| LRA | EBU Tech 3342 の合成信号（ケース 1〜4） | ±1 LU |
| ラウドネス（照合） | 別実装の `ebur128` クレートと、60 秒の合成信号で比較 | I / M / S は ±0.05 LU、LRA は ±0.2 LU |
| トゥルーピーク | サンプル間にピークがあるサイン波（最大 20 kHz、位相 8 通り） | -0.5〜+0.2 dB |
| VU | 立ち上がり時間とオーバーシュート、基準レベル | 300 ms ±10 %、1〜1.7 % |
| PPM | 5 kHz トーンバースト応答と降下時間（Type I / II） | バーストは ±0.5〜1 dB、降下時間は ±0.1 s |

EBU が配布する公式のテスト音源一式による確認は、まだ行っていません。

## 自動テスト

`tools/host-tests/run.sh` は、次の 4 つを順に実行します。

| 手順 | 内容 |
|---|---|
| bundle | リリースビルドで VST3 / CLAP を作成し、`~/.vst3` と `~/.clap` にリンクします |
| clap-validator | CLAP 版を検証します |
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

Ardour の作業ファイルは `target/host-tests/` に作られ、実行のたびに作り直されます。

## GPU のない環境での動作

エディターは OpenGL 3.2 で描画します。

| 環境 | 動作 |
|---|---|
| sRGB 対応のフレームバッファがない（Xvfb や VNC と Mesa llvmpipe の組み合わせなど） | sRGB なしの設定で開き直し、通常どおり表示します |
| OpenGL 3.2 のコンテキストを作れない（GLX のない X サーバー、GPU のない Windows の仮想マシンなど） | ウィンドウは開きますが描画しません。ホストは終了しません |

画面のない環境では、次のように Xvfb を使って自動テストを実行できます。

```sh
Xvfb :99 -screen 0 1280x800x24 &
DISPLAY=:99 tools/host-tests/run.sh
```

## 手動での確認（Ardour）

自動テストで作られたセッション `target/host-tests/session/pt.ardour` を Ardour で開くと、プラグインを挿したバスがそのまま使えます。

1. Ardour を起動し、セッションを開きます。音声デバイスがない場合は、ALSA の `snd-dummy` モジュールを読み込み、Audio System を ALSA、デバイスを Dummy にします。
2. ミキサー画面で `Bus 1` の `Shirabe Meter Suite` をダブルクリックし、エディターを開きます。
3. 再生すると、テスト信号のピーク（L 約 -4.4 dBFS、R 約 -9.1 dBFS）が表示されます。
