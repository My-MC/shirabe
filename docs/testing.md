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
| ラウドネス | EBU Tech 3341 の合成信号（ケース 1〜6、9、13）を 44.1 / 48 / 88.2 / 96 kHz で再現 | ±0.1 LU |
| I と LRA の操作 | 一時停止中は I と LRA が変わらず、M / S は動き続けること。測定時間のリセット | — |
| LRA | EBU Tech 3342 の合成信号（ケース 1〜4） | ±1 LU |
| ラウドネス（照合） | 別実装の `ebur128` クレートと、60 秒の合成信号で比較 | I / M / S は ±0.05 LU、LRA は ±0.2 LU |
| トゥルーピーク | サンプル間にピークがあるサイン波（最大 20 kHz、位相 8 通り） | -0.4〜+0.2 dB（Tech 3341 と同じ） |
| VU | 立ち上がり時間とオーバーシュート、基準レベル | 300 ms ±10 %、1〜1.7 % |
| VU（IEEE Std 152-1991 §2.3） | 周波数特性（25 Hz〜16 kHz、44.1 / 48 / 96 kHz）、戻り時間、反転誤差 | 0.5 dB / 0.2 dB、300 ms ±10 %、0.5 vu |
| PPM Type I | 5 kHz トーンバースト応答（10 / 5 / 3 ms）と降下時間 | バーストは ±0.5〜1 dB、降下時間は ±0.1 s |
| PPM Type II | EBU Tech 3205 表 2 のバースト応答（100 / 10 / 5 / 1.5 / 0.5 ms）、降下時間、行き過ぎ | Tech 3205 の許容範囲 |
| PPM Type II（IEEE Std 152-1991 §1.3） | 周波数特性、積分時間、遅れ時間、過負荷特性、パルス応答、反転誤差 | IEEE 152 の許容範囲 |
| リサージュ | 同相・逆相・90° のステレオ信号、リングバッファの順序と上限、モノラル入力と切り替え | 同相は側成分 0、逆相は中点 0、90° は半径 1/√2（±0.001） |

各値の出典は [standards.md](standards.md) にまとめています。

## EBU の公式テスト音源による適合性テスト

EBU が配布するテスト音源一式（EBU loudness test set v05）で、EBU Tech 3341 と Tech 3342 の全ケースを確認できます。
テスト音源は技術試験にのみ使用でき、再配布できないため、リポジトリには含めず、CI でも実行しません。

テスト音源の zip を EBU のサイトから入手して展開し、展開先を環境変数で指定して実行します。

```sh
SHIRABE_EBU_TESTSET=/path/to/ebu-loudness-test-set \
  cargo test --release -p shirabe-dsp --test ebu_testset -- --ignored --nocapture
```

2026-11-26 の結果は次のとおりで、すべて許容範囲に入りました。

| ケース | 内容 | 期待値と許容範囲 | 結果 |
|---|---|---|---|
| Tech 3341 1〜2 | 1 kHz の正弦波 | M / S / I が -23.0 / -33.0 ±0.1 LUFS | -22.95 / -32.96 |
| Tech 3341 3〜6 | レベルの変化、ゲート、5.0 チャンネル | I が -23.0 ±0.1 LUFS | -23.02〜-22.98 |
| Tech 3341 7〜8 | 実際の番組素材 | I が -23.0 ±0.1 LUFS | -22.99 / -23.00 |
| Tech 3341 9 | S の安定性 | 3 秒以降の S が -23.0 ±0.1 LUFS | 範囲内 |
| Tech 3341 10〜11 | Max S | -23.0 ±0.1、-38.0〜-19.0 ±0.1 LUFS | 範囲内 |
| Tech 3341 12 | M の安定性 | 1 秒以降の M が -23.0 ±0.1 LUFS | 範囲内 |
| Tech 3341 13〜14 | Max M | -23.0 ±0.1、-38.0〜-19.0 ±0.1 LUFS | 範囲内 |
| Tech 3341 15〜23 | トゥルーピーク | -6.0 / +3.0 / 0.0 dBTP、+0.2 / -0.4 dB | 誤差 -0.17〜0.00 dB |
| Tech 3342 1〜6 | LRA | 10 / 5 / 20 / 15 / 5 / 15 ±1 LU | 誤差 0.02 LU 以内 |

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

## PLR の表示

Integrated が確定すると、番組の最大トゥルーピークと Integrated ラウドネスの差を LU で表示します。
1 kHz・-18 dBFS のモノラル正弦波では、トゥルーピークが約 -18 dBTP、Integrated が約 -21 LUFS になり、PLR は約 3 LU になります。
同じ信号を左右に出すステレオのテストオシレーターでは、Integrated が約 -18 LUFS になり、PLR は約 0 LU です。
Pause 中は PLR が保持され、Reset 後は Integrated が求まるまで `--` を表示します。
画面の `TP max` は Pause 中も更新するため、PLR に使う番組の最大値とは異なる場合があります。

## VU の針表示

Level で VU を選び、「Needle」をオンにするとチャンネルごとに針が表示されます。
0 VU の基準とテストオシレーターをともに -18 dBFS に設定すると、針が 0 VU を指します。
「Needle」をオフにすると、計測を続けたままバー表示に戻ります。

## リサージュの確認

テストオシレーターは同じ信号を L/R に出すため、リサージュは縦の直線になります。
位相の違いを見るときは、次のコマンドで WAV ファイルを作り、スタンドアロンアプリの「Source」で `Audio file` を選んで再生します。

```sh
ffmpeg -f lavfi -i "aevalsrc=0.5*sin(2*PI*1000*t)|0.5*sin(2*PI*1000*t):s=48000:d=5" -c:a pcm_f32le /tmp/scope-in-phase.wav
ffmpeg -f lavfi -i "aevalsrc=0.5*sin(2*PI*1000*t)|-0.5*sin(2*PI*1000*t):s=48000:d=5" -c:a pcm_f32le /tmp/scope-anti-phase.wav
ffmpeg -f lavfi -i "aevalsrc=0.5*sin(2*PI*1000*t)|0.5*cos(2*PI*1000*t):s=48000:d=5" -c:a pcm_f32le /tmp/scope-quadrature.wav
ffmpeg -f lavfi -i "aevalsrc=1.5*sin(2*PI*1000*t)|1.5*sin(2*PI*1000*t):s=48000:d=5" -c:a pcm_f32le /tmp/scope-clip.wav
```

期待される表示は次のとおりです。

| ファイル | 表示 |
|---|---|
| `scope-in-phase.wav` | 中央を通る縦の直線。振幅 0.5 なので、上下それぞれ枠の半分まで届きます |
| `scope-anti-phase.wav` | 中央を通る横の直線。左右それぞれ枠の半分まで届きます |
| `scope-quadrature.wav` | 半径が枠の 35.4 % の円 |
| `scope-clip.wav` | 縦の直線が枠の上下端で止まり、タイトルの右に `CLIP` が表示されます |

モノラルの WAV を再生すると、図の代わりに `MONO` と `no stereo content` が表示されます。

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

## スタンドアロンアプリの入力の確認（Linux）

ALSA のループバックデバイスに試験信号を流し、スタンドアロンアプリで測定します。
44.1 kHz と 96 kHz でも同じ手順で確認でき、デバイスのサンプルレートがそのまま使われることを確かめられます。

次のコマンドで、ループバックデバイスを用意します。

```sh
sudo modprobe snd-aloop
```

次のコマンドで、L -18 dBFS / R -24 dBFS の 1 kHz 正弦波を 44.1 kHz で流します。

```sh
ffmpeg -re -f lavfi -i "aevalsrc=0.125893*sin(2*PI*1000*t)|0.0630957*sin(2*PI*1000*t):s=44100:d=60" -f alsa hw:Loopback,0,0
```

スタンドアロンアプリを起動し、「Audio API」で ALSA、「Input」で `hw:CARD=Loopback,DEV=1` を選びます。
期待される表示は次のとおりです（2026-11-26 に 44.1 kHz と 96 kHz で確認）。

| 項目 | 表示 |
|---|---|
| サンプルレート | 44.1 kHz（または 96.0 kHz） |
| ピーク / トゥルーピーク | L -18.0 / R -24.0 |
| VU | L +0.0 / R -6.0 |
| Integrated | -20.0 LKFS |

## スタンドアロンアプリの再生音の確認

### Linux（PulseAudio / PipeWire）

PulseAudio に仮想の出力を作り、そこに試験信号を流して「Playing on this computer」の項目で測定します。

次のコマンドで、44.1 kHz の仮想出力を作ります。

```sh
pactl load-module module-null-sink sink_name=testsink rate=44100 sink_properties=device.description=TestSink
```

次のコマンドで、L -18 dBFS / R -24 dBFS の 1 kHz 正弦波を仮想出力に流します。

```sh
ffmpeg -re -f lavfi -i "aevalsrc=0.125893*sin(2*PI*1000*t)|0.0630957*sin(2*PI*1000*t):s=44100:d=60" -f pulse -device testsink shirabe-test
```

「Source」で `TestSink (playing)` を選ぶと、入力の確認と同じ値（ピーク L -18.0 / R -24.0、VU L +0.0 / R -6.0、Integrated -20.0 LKFS）になります。
再生を止めると、VU と Momentary は -inf に戻ります（2026-11-26 に確認）。

### Windows（WASAPI ループバック）

「Audio API」で WASAPI を選ぶと、出力デバイスが `(playing)` 付きで表示されます。
2026-11-26 に、音声デバイスのない Windows 11 の仮想マシンで、リモートデスクトップの「リモート オーディオ」（44.1 kHz）を使って次のことを確認しました。

| 確認したこと | 結果 |
|---|---|
| L -18 / R -24 dBFS の WAV を Windows 標準の機能で再生 | ピーク L -18.0 / R -24.0、VU L +0.0 / R -6.0、Integrated -20.0 LKFS |
| 再生が止まったあと | VU と Momentary が -inf に戻る |
| 日本語のデバイス名 | OS の日本語フォントで表示 |

### macOS

macOS 版はビルドが通ることだけを CI で確認しています。
実機での確認は済んでいません。

## スタンドアロンアプリのファイル再生の確認

スタンドアロンアプリで WAV / FLAC ファイルを再生し、メーターと出力の両方を確認します。

### 自動テスト

ファイルのデコード、メーターへの渡し方、サンプルレート変換、リングバッファは `cargo test` で検証します。

```sh
cargo test -p shirabe-app
```

| 対象 | 内容 | 許容範囲 |
|---|---|---|
| デコード（WAV） | 8 / 16 / 24 / 32 bit PCM、32 bit float、モノラル / ステレオ | サンプル値が一致 |
| デコード（FLAC） | 16 bit ステレオのテスト音源（`crates/shirabe-app/testdata`） | ピークと RMS が ±0.002 |
| メーターのレート | 44.1 kHz のファイルを 48 kHz のデバイスで再生 | メーターのレートは 44.1 kHz、ピークは -18.0 / -24.0 dBFS |
| デバイスのレートの影響 | 同じファイルを 44.1 kHz と 48 kHz で再生 | ピークの差は 0.001 dB 未満 |
| サンプルレート変換 | 44.1 ↔ 48 kHz、96 → 48 kHz のサイン波 | 通過帯域は ±0.2 dB、折り返しは -57 dB 以下 |
| リングバッファ | 書き込みと読み出しを別スレッドで実行 | 2 万フレームが順序どおりに届くこと |
| 出力バッファ（Linux） | 8 / 22.05 / 32 / 44.1 / 48 / 88.2 / 96 / 192 kHz の固定バッファ | 周期の整数倍（4 の倍数）で、50 ms との差が 1 ms 未満 |

### Linux（ALSA のループバック）

`snd-aloop` のループバックで、アプリが実際に出力した音を取り出して確認します。
`hermes` は `audio` グループに入っていないため、`sg audio -c` で実行します。

次の内容で ALSA の設定ファイルを作り、`default` をループバックにつなぎます。

```sh
cat > /tmp/shirabe-asoundrc <<'CONF'
</usr/share/alsa/alsa.conf>
pcm.!default {
    type plug
    slave { pcm "hw:Loopback,0,0" }
}
ctl.!default { type hw card Loopback }
CONF
```

次のコマンドで、ループバックの取り出し側を 10 秒録音します。

```sh
sg audio -c 'ALSA_CONFIG_PATH=/tmp/shirabe-asoundrc ffmpeg -f alsa -i hw:Loopback,1,0 -t 10 -ac 2 -ar 44100 capture.wav'
```

録音を始めたら、アプリを同じ設定で起動します。

```sh
sg audio -c 'ALSA_CONFIG_PATH=/tmp/shirabe-asoundrc \
  XDG_CONFIG_HOME=/tmp/shirabe-config ./target/release/shirabe'
```

「Source」で `Audio file` を選び、`File` にファイルのパスを入れて `Play` を押します。
2026-11-26 に、次の結果を確認しました。

| 確認したこと | 結果 |
|---|---|
| L -18 / R -24 dBFS、1 kHz、44.1 kHz の WAV の再生 | 録音は 4.0 秒、L -18.0 / R -24.0 dBFS、1 kHz。途中の無音なし |
| 同じ内容の FLAC の再生 | 録音は 4.0 秒、L -18.0 / R -24.0 dBFS |
| -12 dBFS のモノラルの WAV の再生 | 左右が同じ波形で -12.0 dBFS、画面は「Mono」 |
| 一時停止と再開 | 2.0 秒再生 → 3.8 秒無音 → 2.0 秒再生 |
| メーターの表示 | ピーク L -18.0 / R -24.0、VU L +0.0 / R -6.0、Integrated -20.0 LKFS |

確認のときの注意点は次のとおりです。

- 録音は `hw:Loopback,1,0` で行い、アプリの出力は `default`（ループバックの再生側）にします。
- 録音とアプリは同じサンプルレートで開きます。
- ALSA の `plug` がレートを変換するため、この構成ではアプリ自身のサンプルレート変換は動きません。
  変換の確認は自動テストで行います。
- デバッグビルドは最適化されていないため、実時間で再生できず、音が途切れます。
  音の確認には `--release` を使ってください。デバッグビルドは、メモリ確保の検出（`assert_no_alloc`）に使います。

### Windows と macOS

ファイル再生の Windows / macOS での確認はまだ行っていません。
WASAPI と Core Audio では、デバイスがファイルのレートを受け付けない場合にアプリ側で変換します。
