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
| ステレオ相関・左右バランス | 同相・逆相・90°、左右のレベル差、片側だけ、無音、モノラル、入力の切り替え、可変ブロック長 | 相関 +1・-1・0、振幅比 2:1 は 6.0 dB、片側だけの場合は相関なし |
| リサージュ | 同相・逆相・90° のステレオ信号、リングバッファの順序と上限、モノラル入力と切り替え | 同相は側成分 0、逆相は中点 0、90° は半径 1/√2（±0.001） |
| リサージュの残光 | 0.3 秒を過ぎた点の削除、上限 16384 点、新しいサンプルがないときに増えないこと、モノラル・入力リセットでの消去 | 時間と件数の上限、増加なし |
| リサージュの自動ゲイン | 最大 +20 dB、-60 dBFS 未満での保持、無音時の非上昇、定常入力での非振動、固定／自動の表示 | ±0.1 dB、振動幅 0.1 dB 未満 |
| ベクタースコープ | 横軸が L、縦軸が R になること。同相・逆相・非対称なステレオ信号、モノラル入力 | 非対称（L 0.5 / R 0.25）は傾き 0.5、逆相は傾き -1、同相は傾き +1 |
| ベクタースコープの切り替え | 表示の切り替えで履歴と自動ゲインが初期化され、切り替え後の座標で描き直されること。計測値と出力が変わらないこと | 履歴に前の表示の点が残らない。レベル、VU、ラウドネス、出力サンプルが一致 |
| 表示言語 | 英語と日本語の文言がそろっていること、技術名と単位が変わらないこと、日本語の全文字がフォントで描けること | 空の文言なし。日本語の未翻訳は規格名と単位だけ。日本語の全文に字形がある |
| 表示言語の配置 | 640×380 の最小サイズで、両言語のヘッダー、全タイル、読み出しの全行、LRA の注記、設定行、オシレーターの状態文が重ならずに収まること | 文字が矩形の内側にあり、行が重ならない。192 kHz の表示でもヘッダーが収まる |
| 表示言語の状態 | `ui_lang` が英語の初期値で、自動化できず、末尾に追加されていること。古い設定と言語の往復で他の値が変わらないこと | 既存 ID の位置が不変。`ui_lang` のない設定は英語。往復後もレイアウトと目標値が同じ |
| ラウドネスの履歴 | 100 ms ごとの記録（ブロック 1〜4800 フレーム）、1800 点の上限と古い点の破棄、Pause 中の停止と再開、値のない点 | 10 点/秒、上限 1800 点、Pause 中は増えない |
| ラウドネスの履歴（エンジン） | 実際の `MeterEngine` での記録、Pause と再開、Reset、サンプルレートとチャンネル数の変更、デバッグビルドのメモリ確保の検出 | 測定と同じ 100 ms 間隔。変更で消去。確保なし |
| ラウドネス履歴の表示 | 縦横の座標変換、目標値と許容範囲の帯、値のない区間の切れ目、小さいタイルでの見出しのみ、最小サイズでの数値の行 | 右端が最新、目標値が 0 LU、行が欠けない |
| スペクトラム（正弦波） | ビン中心の正弦波を 44.1 / 48 / 96 kHz で、100 Hz / 1 kHz / 10 kHz、-6 / -20 dBFS | 周波数は ±1 ビン、レベルは ±0.1 dB |
| スペクトラム（ビン間） | ビンの中間にある 1 kHz の正弦波 | 周波数は ±1 ビン、レベルは Hann 窓の最大損失（約 1.4 dB）の範囲内 |
| スペクトラム（窓の正規化） | 振幅 1.0 / 0.5 / 0.1 のビン中心正弦波 | 0 / -6.02 / -20 dBFS、±0.05 dB |
| スペクトラム（帯域） | 白色雑音とピンク雑音を 12 秒 | 白色は 1 オクターブあたり約 +3 dB（周波数が 10 倍で +10 dB、±2 dB）、ピンクは ±1.5 dB で平坦 |
| スペクトラム（ナイキスト以上） | 32 kHz で 20 kHz バンドを含む白色雑音 | 20 kHz バンドは -150 dB。16 kHz バンドはナイキストまでのビンだけを使う |
| スペクトラム（取り込み） | リングの順序と上限、半窓ごとの更新、クリア、モノラル合計 | 新しいフレームが残る。更新は半窓ごと。クリアで初期化。片側だけの信号は 6 dB 低い |
| スペクトラム（エンジン） | ステレオのモノラル合計、モノラル入力、片側だけ、Reset、デバッグビルドのメモリ確保 | 両側の正弦波は 0 dBFS、片側だけは -6 dB。Reset で消去。確保なし |
| スペクトラムの表示 | 対数周波数軸、0〜-90 dBFS、バンドの並び、小さいタイル、FFT と RTA の見出し、音声待ち | 1 kHz が 20 Hz〜20 kHz の 56.6 %、0 dBFS が上端。小さいタイルは見出しだけ |
| サイドチェーン比較（出力） | 補助入力に別の信号を入れても、メイン出力が補助入力のない実行とサンプル単位で一致すること | すべてのブロックでビット一致 |
| サイドチェーン比較（差分） | 6.02 dB と 12.04 dB 異なる 2 信号、同一信号（ステレオとモノラル）、無音のリファレンス | 差分は ±0.15 LU、同一信号は 0 LU、無音は `--` |
| サイドチェーン比較（操作） | Reset と Pause がメインとリファレンスの I に同じように働き、M / S は動き続けること | Pause 中は I が不変、Reset 後は両方が `--` |
| サイドチェーン比較（レイアウト） | `AUDIO_IO_LAYOUTS` の先頭 2 つが従来のステレオとモノラルで、補助入力つきの 2 つが末尾にあること。補助出力がないこと | 既存のバス構成が不変。`lay_preset` の ID は 5 つで、既存の 4 つの意味も不変 |
| サイドチェーン比較（CPU・確保） | デバッグビルドのメモリ確保の検出。release ビルドでの 1 フレームあたりの処理時間 | 確保なし。リファレンスの追加は 1 フレームあたり約 0.02 µs（48 kHz ステレオ） |
| サイドチェーン比較の表示 | Compare レイアウトの配置、差分の行と符号、`--` の表示、補助入力がないときの案内、最小サイズでの後退 | 900×560 で 3 行が数値の下に収まる。640×380 では Reference を隠し、数値の行は残る |

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

## サイドチェーン比較の確認

プラグインの補助入力（ホストのサイドチェーン）にリファレンス音源をつなぎ、メインとの差分が表示されることを確認します。
計測と表示は次の自動テストで確認できます。

```sh
cargo test -p shirabe-plugin --test reference_comparison
cargo test -p shirabe-plugin the_compare_layout
cargo test -p shirabe-ui --test layout_presets
cargo test -p shirabe-ui --test meter_views
```

CPU の目安は、release ビルドで次のコマンドを実行して測ります。

```sh
cargo test --release -p shirabe-plugin --test reference_comparison -- --ignored --nocapture
```

2026-11-26 にこの VM（8 vCPU）で測った結果は次のとおりです。

| 構成 | 1 フレームあたり | 48 kHz の 1 コアに占める割合 |
|---|---|---|
| リファレンスなし | 0.479 µs | 2.3 % |
| リファレンスあり | 0.494 µs | 2.4 % |

リファレンスの追加は 0.015 µs（約 3 %）でした。
数値はこの環境の release ビルドでの目安で、保証値ではありません。

### ホストでの見え方

補助入力は `AUDIO_IO_LAYOUTS` の末尾に追加しているため、ホストによって見え方が異なります。
確認した結果は次のとおりです。

| 確認 | 結果 |
|---|---|
| clap-validator（`layout-audio-ports-config`） | 4 つのポート構成をすべて選び、音声を通して成功（44 件中 34 件成功、10 件スキップ、失敗 0） |
| pluginval（VST3、strictness 5） | 成功。バスの一覧にはメインのステレオバスだけが現れ、補助入力は現れない |
| Ardour（VST3）のパススルー | ビット一致（従来の確認と同じ） |

CLAP では、ホストのポート構成の選択に「Stereo with sidechain」と「Mono with sidechain」が並びます。
選ぶと `Reference` ポートが現れます。
そのポートにリファレンス音源を送ると、Compare レイアウトに差分が表示される想定です。

VST3 では補助入力が現れません。
nih-plug は使用中のレイアウトのバスだけをホストに通知し、初期状態のレイアウトは先頭のステレオ（補助入力なし）だからです。
既存プロジェクトのバス構成を変えないことを優先した結果で、対応するには先頭レイアウトの変更、別プラグインとしての登録、ラッパーの変更のいずれかが必要です（[design.md](design.md)）。

Ardour で補助入力を実際に配線する確認は、VST3 に補助入力が現れないため行っていません。
CLAP に対応したホストでの配線確認は未実施です。

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

### 残光と自動ゲインの確認

残光は初期状態でオンです。
スコープの設定にある「Trail」をオフにすると、直近のトレースだけを表示します。
「Auto」をオンにすると、固定スケールの代わりに適用中のゲインを表示します。

時間とともに形が変わる図を作るには、次のコマンドで 1 kHz と 1.002 kHz のステレオ WAV を作ります。

```sh
ffmpeg -f lavfi -i "aevalsrc=0.4*sin(2*PI*1000*t)|0.4*sin(2*PI*1002*t):s=48000:d=12" -c:a pcm_f32le /tmp/scope-rotate.wav
```

自動ゲインの確認には、次のコマンドで -34 dBFS 程度の小さいステレオ WAV を作ります。

```sh
ffmpeg -f lavfi -i "aevalsrc=0.02*sin(2*PI*1000*t)|0.02*sin(2*PI*1000*t):s=48000:d=12" -c:a pcm_f32le /tmp/scope-quiet.wav
```

スタンドアロンアプリでファイルを再生し、次の点を確認します。

| 操作 | 期待される表示 |
|---|---|
| 残光オンで `scope-rotate.wav` を再生 | 明るい楕円の外側に、回転してきた図が薄く重なって見える |
| 残光オフで同じファイルを再生 | 直近の図だけが表示され、外側の薄い図は消える |
| 自動ゲインオンで `scope-quiet.wav` を再生 | タイトル右が `AUTO +19.6 dB` のような表示になり、図が拡大される |
| 再生を一時停止 | 残光が 0.3 秒で消え、`AUTO +19.7 HOLD` のように最後のゲインと `HOLD` が残る |
| 固定スケールに戻す | タイトル右が `FIXED ±1` に戻り、図の大きさが元に戻る |

2026-11-26 に Linux（Xvfb、ALSA の `null` 出力）で確認した結果は次のとおりです。

| 項目 | 結果 |
|---|---|
| 固定スケール | `FIXED ±1` を表示し、-30 dBFS のテストトーンが縦線で表示された |
| 残光 | オンでは現在の楕円の外側に薄い弧が残り、オフでは消えた |
| 自動ゲイン | `scope-quiet.wav` で 5 秒後に `AUTO +19.6 dB` を表示し、図が拡大された |
| 一時停止 | `AUTO +19.7 HOLD` を表示し、残光は 0.3 秒で消えた |
| レイアウト | 820×760 でラウドネスの全行とリサージュが同時に表示された |

スタンドアロンアプリでファイル再生の行が増えると、820×560 ではメーター領域が 333 論理ピクセルを下回り、リサージュは隠れます。
その場合はウィンドウを縦に広げます。

### ベクタースコープの確認

「L/R」をオンにすると、同じフレームを横軸 L、縦軸 R で表示します。
軸の向きは、左右で振幅の異なる信号と逆相の信号を使うと確かめられます。

```sh
ffmpeg -f lavfi -i "aevalsrc=0.5*sin(2*PI*1000*t)|0.25*sin(2*PI*1000*t):s=48000:d=10" -c:a pcm_f32le /tmp/scope-asymmetric.wav
ffmpeg -f lavfi -i "aevalsrc=0.5*sin(2*PI*1000*t)|-0.5*sin(2*PI*1000*t):s=48000:d=10" -c:a pcm_f32le /tmp/scope-anti-phase.wav
```

| ファイル | M/S 表示 | L/R 表示 |
|---|---|---|
| `scope-asymmetric.wav`（L 0.5 / R 0.25） | 傾き 3 の直線（M = 0.375、S = 0.125） | 傾き 0.5 の直線（L が横、R が縦） |
| `scope-anti-phase.wav` | 中央を通る横の直線 | 左上から右下へ傾き -1 の直線 |

2026-11-26 に Linux（Xvfb、ALSA の `Dummy` 出力）のスタンドアロンアプリで確認した結果は次のとおりです。

| 項目 | 結果 |
|---|---|
| 軸の向き（非対称） | M/S は傾き 2.96、L/R は傾き 0.50。画面のピクセルから測った値で、期待値と一致 |
| 軸の向き（逆相） | M/S は傾き 0.00 の横線、L/R は傾き -1.00 の直線 |
| 表示の切り替え | 30 fps で画面を記録し、M/S から L/R へ切り替えた最初のフレームに横線の履歴が残っていないことを確認 |
| 設定の保存 | `standalone.json` の `scp_lr` が 0 と 1 で保存され、再起動後も復元 |
| 最小サイズ（640×440） | 「L/R」「Trail」「Auto」が欠けずに表示され、ラウドネスの行も残る |

### レイアウトプリセットの確認

上部の「Layout」で Standard と Stereo を切り替え、計測を止めずに配置だけが変わることを確認します。
テストオシレーターを -18 dBFS で有効にすると、VU が 0 VU、Integrated が -18.0 LKFS のまま推移します。

| 操作 | 期待される表示 |
|---|---|
| Standard で起動 | Peak、Level、Loudness が横に並び、右列の上に数値のラウドネス、下にリサージュ |
| Stereo に切り替え | Peak と Level が左に縦積み、中央に高さいっぱいのリサージュ、右に Loudness と数値のラウドネス |
| Standard に戻す | 元の配置に戻り、Integrated と Time は切り替え前から続いている |
| 640×440 に縮小（Standard） | リサージュは隠れ、数値のラウドネスの全行と Time、Target は残る |
| 640×440 に縮小（Stereo） | Loudness のバーは隠れ、数値のラウドネスの全行と Time、Target は残る。リサージュは表示される |
| 設定の保存 | `standalone.json` の `lay_preset` が 0（Standard）と 1（Stereo）で保存され、再起動後も復元される |

2026-11-26 に Linux（Xvfb :99、ALSA、Source: None、テストオシレーター -18 dBFS）のスタンドアロンアプリで確認した結果は次のとおりです。

| 項目 | 結果 |
|---|---|
| Standard → Stereo → Standard | 配置が切り替わり、Integrated は -18.0 LKFS のまま、Time は 0:59 → 1:35 と進んだ |
| Stereo の配置 | Peak が左上、VU が左下、リサージュが中央、Loudness の M / S バーと数値のラウドネスが右に並んだ |
| 640×440（Standard） | リサージュは消え、Integrated、LRA、PLR、M max、S max、TP max、Time、Target の全行が表示された |
| 640×440（Stereo） | Loudness のバーは消え、数値のラウドネスの全行とリサージュが表示された |
| 上部の操作 | 640×440 でも「Layout」、Pause、Reset、および Level 行の L/R、Trail、Auto が欠けずに表示された |
| 設定の保存 | `lay_preset` が 1.0 → 0.0 → 1.0 と保存され、再起動後に Stereo で起動した |
| Linux ホスト（`tools/host-tests/run.sh`） | clap-validator 44 件（34 件成功、10 件スキップ、失敗 0）、pluginval strictness 5（エディターを含む）が成功、Ardour のパススルーがビット一致 |

### スペクトラムの確認

スタンドアロンアプリで、Layout を Spectrum に切り替えます。
テストオシレーターを -18 dBFS で有効にし、周波数を 63 Hz、125 Hz、1 kHz、8 kHz に変えます。
FFT 表示と 1/3 oct 表示を切り替え、ピークとバンドが周波数に追従することを確認します。

| 操作 | 期待される表示 |
|---|---|
| Spectrum を選ぶ | Standard と同じ 3 列と数値のラウドネスの下に、幅いっぱいのスペクトラムが入る |
| FFT 表示、1 kHz、-18 dBFS | 1 kHz の位置に線が立ち、右上のピークが約 1.00 kHz / -18.0 dBFS |
| 1/3 oct 表示、1 kHz | 1 kHz のバンドが -18 dB 付近まで伸びる |
| 63 Hz、125 Hz、8 kHz | ピークとバンドがそれぞれの周波数へ移る |
| Reset | スペクトラムが消え、音声が届くと再び表示される |
| 640×440 に縮小 | スペクトラムが隠れ、数値のラウドネスの全行が残る |
| 再起動 | `standalone.json` の `lay_preset` と `spc_mode` が復元される |

2026-11-26 に Linux（Xvfb :99、ALSA、Source: None、テストオシレーター -18 dBFS、48 kHz）で確認した結果は次のとおりです。

| 項目 | 結果 |
|---|---|
| Spectrum を選ぶ | Standard と同じ 3 列と数値のラウドネスの下に、幅いっぱいのスペクトラムが表示された |
| FFT 表示、1 kHz、-18 dBFS | 1 kHz に線が立ち、右上は 999 Hz / -18.6 dBFS だった（ビン間のため約 0.6 dB 低い） |
| 1/3 oct 表示、1 kHz | 1k のバンドだけが -18 dB 付近まで伸び、ほかのバンドは下にあった |
| 63 Hz | 64 Hz / -19.4 dBFS |
| 125 Hz | 126 Hz / -18.6 dBFS |
| 8 kHz | 8.00 kHz / -18.6 dBFS |
| 640×440 に縮小 | スペクトラムは隠れ、Integrated、LRA、PLR、M max、S max、TP max、Time、Target の全行と 1/3 oct の選択は残った |
| 1/3 oct → FFT の切り替え | 画面の選択が変わり、`standalone.json` の `spc_mode` が 1.0 から 0.0 に保存された |
| 再起動 | `lay_preset` が 3.0（Spectrum）で保存され、Spectrum で起動した |
| Reset | 自動テストで確認した。画面では消去から再描画までが約 171 ms のため目視できなかった |
| Linux ホスト（`tools/host-tests/run.sh`） | clap-validator 44 件（34 件成功、10 件スキップ、失敗 0）、pluginval strictness 5（エディターを含む）が成功、Ardour のパススルーがビット一致 |

### 表示言語の確認（日本語 / 英語）

ヘッダーの「EN / JA」を開き、English と 日本語 を切り替えます。
テストオシレーターを -18 dBFS で有効にし、切り替えの前後で Integrated、Time、各バーが続くことを確認します。
日本語の文字が空の四角にならず、640×440 でも読み出しの全行と設定行が欠けないことを確認します。

| 操作 | 期待される表示 |
|---|---|
| 初回起動 | 英語で表示し、ヘッダーに「EN」が出る |
| EN → 日本語 | 見出し、ボタン、ツールチップ、読み出しの項目名、設定行、スタンドアロンのデバイス・再生・外観画面が日本語になる。Integrated と Time は続く |
| 日本語 → English | 元の英語表示に戻る。Integrated と Time は続く |
| 設定の保存 | `standalone.json` の `ui_lang` が 0（英語）と 1（日本語）で保存され、再起動後も復元される |
| 640×440、Custom、日本語 | 読み出しの全行と LRA の注記、Custom の行、目標と Time が表示され、文字が欠けない |
| フォントのない環境（想定） | 日本語を選んでも英語で表示する。以後は日本語を無効にし、理由をツールチップに表示する |

2026-11-26 に Linux（Xvfb :97、ALSA、Source: None、テストオシレーター -18 dBFS）のスタンドアロンアプリで確認した結果は次のとおりです。

| 項目 | 結果 |
|---|---|
| 英語 → 日本語 | ヘッダー、タイル見出し、読み出し、設定行、デバイス・外観画面が日本語になった。Integrated は -18.0 LKFS のまま、Time は 0:30 → 0:47 と進んだ |
| 日本語 → 英語 | すべて英語に戻り、Integrated は -18.0 LKFS のまま、Time は 0:47 → 1:08 と進んだ |
| 文字の表示 | 日本語の文字は空の四角にならず、「日本語」のメニュー項目も正しく表示された |
| 設定の保存 | `ui_lang` が 1.0 → 0.0 → 1.0 と保存された |
| 再起動 | 日本語のまま起動した |
| 640×440、日本語、Custom | 統合ラウドネス、LRA、PLR、M 最大、S 最大、TP 最大、時間、目標と LRA の注記、Custom 行、レベル行、オシレーター行がすべて表示された |
| 640×440、日本語、オシレーター有効 | 「テスト音を入力中」が右端で切れず、スライダーの値も欠けなかった |

自動テストは、言語の表、最小サイズの配置、フォントの字形、古い設定の読み込みを確認します。

```sh
cargo test -p shirabe-ui --test i18n
cargo test -p shirabe-plugin --test ui_language
cargo test -p shirabe-plugin --test fonts
cargo test -p shirabe-app context
```

`shirabe-ui` の `layout` モジュールは、タイルの重なりと画面外へのはみ出しを自動テストで確認します。

```sh
cargo test -p shirabe-ui --test layout_presets
```

タイルの描画の登録表は、識別子の一意性、各プリセットが置くタイルの引き当て、描画の順序と矩形を自動テストで確認します。

```sh
cargo test -p shirabe-ui --test meter_views
cargo test -p shirabe-plugin the_editor_registers_every_tile_once
```

登録表を導入しても描画が変わっていないことは、導入前と導入後のスタンドアロンアプリを同じ設定（Source: None、テストオシレーター -18 dBFS、820×560）で起動し、画面を撮影して比べました。
Standard と Stereo のどちらも、導入前のビルドの画面と 1 ピクセルも違いませんでした。
Standard ではプログラムタイマーと残光が実行ごとにわずかに変わるため、導入前のビルドを 2 回撮影した画面とも一致することを確認しています。

### ラウドネス履歴の確認

上部の「Layout」で History を選ぶと、画面の下端にラウドネスの履歴が表示されます。
上段のメーターと数値の配置は Standard と同じです。
テストオシレーターを -30 dBFS で有効にし、1 分ほど記録してからレベルを動かすと、段差を確認できます。

| 操作 | 期待される表示 |
|---|---|
| History を選ぶ | 下端に「LOUDNESS HISTORY」「3 min」と 3 分間のグラフが現れる。上段は Standard と同じ配置 |
| 記録が 10 秒程度 | 右端に短い線だけが現れる |
| オシレーターを -30 dBFS から -8.5 dBFS へ | 段差が現れ、上の区間が約 -15 LKFS になる |
| Pause | 見出しが「LOUDNESS HISTORY PAUSED」（赤）になり、線が止まる。M / S のバーは動き続ける |
| Continue | 記録が止めた位置から伸びる |
| Reset | グラフが消え、右端から記録し直す |
| 最小サイズに縮小 | 数値のラウドネスの全行と Time、Target が残る。高さが足りないときはグラフを隠す（自動テストでは 640×380 で確認） |
| EN → 日本語 | 「ラウドネス履歴」「3 分」「秒前」になる |
| 設定の保存 | `standalone.json` の `lay_preset` が 2 で保存され、再起動後も History で起動する |

2026-11-26 に Linux（Xvfb :96、ALSA、Source: None、テストオシレーター -30 dBFS）のスタンドアロンアプリで確認した結果は次のとおりです。

| 確認したこと | 結果 |
|---|---|
| グラフの表示 | 「LOUDNESS HISTORY」「3 min」、LKFS の目盛り、-120 / -60 秒の目盛り、目標値 -24 LKFS の線と ±1 LU の帯、M / S の凡例を表示した |
| レベルの変化 | -30 dBFS から -8.5 dBFS に上げると、約 -30 LKFS の平坦な線から約 -15 LKFS への段差が現れた |
| Pause | 見出しが「LOUDNESS HISTORY PAUSED」になり、線が止まった。M / S の数値は動き続けた |
| 日本語 | 「ラウドネス履歴」「3 分」「秒前」を表示した。文字は欠けなかった |
| メモリ確保 | デバッグビルドで動作させ、オーディオスレッドの確保による停止は起きなかった |
| 設定の保存 | `lay_preset` が 2.0、`osc_lvl` が -8.5 で保存された |
| 最小サイズ | 自動テストで確認した（640×380 でグラフを隠し、数値の行を維持する） |

自動テストは次のコマンドで実行できます。

```sh
cargo test -p shirabe-dsp --test loudness_history
cargo test -p shirabe-plugin --test loudness_history
cargo test -p shirabe-ui --test history_graph
cargo test -p shirabe-ui --test layout_presets
```

### 外観プリセットファイルの確認（スタンドアロン）

スタンドアロンアプリの「Appearance」を開き、レイアウトとテーマ色をファイルに書き出し、読み込みます。
設定を分離するため、`XDG_CONFIG_HOME` に空のディレクトリを指定して起動します。

```sh
XDG_CONFIG_HOME=/tmp/shirabe-appearance DISPLAY=:99 ./target/debug/shirabe
```

「Source」を `None` にし、テストオシレーターを -18 dBFS で有効にすると、VU が 0 VU、Integrated が -18.0 LKFS のまま推移します。

| 操作 | 期待される動作 |
|---|---|
| 新しいパスで Export | 指定したファイルを作り、形式、バージョン、レイアウト名、テーマ色だけを書く |
| 同じパスでもう一度 Export | 上書きせず、「Overwrite」と「Cancel」を表示する。Cancel でファイルは変わらない |
| 確認中にパスを編集する、または（失敗する）Import を行う | 確認を取り消し、改めて Export するまで Overwrite を表示しない |
| レイアウトを Stereo、背景色を変えたファイルを Import | 配置と色だけが変わり、Integrated などの測定値は続く |
| Import 後にアプリを再起動 | 読み込んだレイアウトと色で起動する |
| 不正な色のファイルを Import | エラーを表示し、レイアウト、色、`standalone.json` は変わらない |
| 読み込み成功後に `standalone.json` に書けない | 外観は変わり、次回起動時に維持されない可能性を警告する |
| 640×440 に縮小 | 「Appearance」ボタンと、ラウドネスの全行、Time、Target が欠けない |

Linux（Xvfb、ALSA、Source: None、テストオシレーター -18 dBFS）のスタンドアロンアプリで確認した結果は次のとおりです。

| 確認したこと | 結果 |
|---|---|
| 初回の Export | `level_bar.normal` と `level_bar.over` を含むバージョン 1 の JSON を作成した |
| 既存ファイルへの Export | 対象パスを確認画面に表示し、明示的な Overwrite まで既存の内容は不変だった |
| パス編集と不正な JSON の Import | 確認が消え、元のファイルと設定ファイルは変わらなかった |
| Stereo と独自色の Import | 配置が Stereo に変わり、背景 `#201018` とシアンの VU バーが反映された。Integrated は -18.0 LKFS のままだった |
| Import 後の `standalone.json` | `lay_preset` が 1.0、`theme.background` が `#201018` になった。`source` は `None`、`osc_on` は 1.0 のままだった |
| 再起動 | Stereo と背景色、メーター色が復元された |
| 設定保存の失敗 | 分離した設定先をディレクトリに置き換えると、外観は反映され、設定を保存できない旨が表示された |
| ワーカーの待機中 | 分離した設定先を名前付きパイプに置き換えて I/O 待ちを確認した。待機中も「Appearance」を閉じられ、Integrated は -18.0 LKFS のまま Time が進んだ |
| 640×440 | 「Appearance」、Integrated、LRA、PLR、M max、S max、TP max、Time、Target が表示された |

自動テストでは、不正な色やバーの閾値の拒否、64 KiB の上限、既存・特殊ファイルの保護、上書き確認の取り消し、設定の互換性と書き込みエラー、処理中のワーカーへの追加要求の拒否と完了通知も確認します。
スタンドアロンの関連テストは次のコマンドで実行できます。

```sh
cargo test -p shirabe-app appearance
cargo test -p shirabe-app settings
cargo test -p shirabe-app file_io
```

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

## 測定結果の書き出しの確認

スタンドアロンアプリの「Export measurement」で、概要と履歴が 1 つのファイルになることを確認します。

### 自動テスト

書式、欠測値、スキーマ、CSV の引用、スナップショット、ファイル操作、表示言語は `cargo test` で検証します。

```sh
cargo test -p shirabe-app -p shirabe-ui
```

| 対象 | 内容 |
|---|---|
| 数値の書式 | 小数点に `.` を使い、レベルは 2 桁、番組時間と履歴の時刻は 3 桁になること |
| 欠測値 | JSON では `null`、CSV では `value` が空欄になること |
| スキーマ | JSON の `schema` が `shirabe-measurement/1` であること |
| CSV の引用 | コンマ、引用符、改行を含む文字列が引用符で囲まれ、引用符が 2 つ重なること |
| CSV の形 | すべての行が 6 列で、履歴の時刻が 0 から 0.1 秒ごとに進むこと |
| スナップショット | `SharedMeters` の値、チャンネルごとの最大トゥルーピーク、目標値、履歴を押した時点でコピーすること |
| 入力の表記 | 音声ファイルは既定で名前だけ、指定したときだけフルパスになること |
| 時刻 | UTC の ISO 8601 になり、うるう年と年末年始をまたいでも日付が正しいこと |
| ファイル操作 | 新規は排他的に作成し、確認なしでは既存ファイルを置き換えないこと |
| 表示言語 | 書き出し画面の文字が日本語と英語の両方にあること |

### Linux（テストオシレーター）

`Xvfb` と分離した `XDG_CONFIG_HOME` でアプリを起動し、入力 `None` とテストオシレーター（-18 dBFS、1 kHz）で確認します。

```sh
mkdir -p /tmp/shirabe-export-test/shirabe
cat > /tmp/shirabe-export-test/shirabe/standalone.json <<'EOF'
{"version":1,"source":"None","params":{"osc_on":1.0}}
EOF
Xvfb :99 -screen 0 1280x800x24 +extension GLX +render -noreset &
DISPLAY=:99 XDG_CONFIG_HOME=/tmp/shirabe-export-test ./target/debug/shirabe &
```

`Export measurement` を開き、次の順に操作します。

1. CSV のまま `Export` を押し、`/tmp/shirabe-export-test/shirabe/measurement-export.csv` を読む。
2. 形式を JSON に変え、パスが `measurement-export.json` に変わることを確認して `Export` を押す。
3. もう一度 `Export` を押し、確認の表示が出てから `Overwrite` を押す。
4. 表示言語を日本語に変え、見出しと確認の文が日本語になることを確認する。

2026-11-26 に、次の結果を確認しました。

| 確認したこと | 結果 |
|---|---|
| CSV の概要 | `schema`、`app_version` 0.1.0、`exported_at` 2026-11-26T12:24:33Z、`integrated_lufs` -17.99、`max_true_peak_dbtp` L / R -18.00、`target_preset` arib |
| CSV の履歴 | 0.000 秒から 30.700 秒まで 0.1 秒ごと。測定開始直後の M / S は空欄 |
| JSON | `schema` が `shirabe-measurement/1`、履歴 496 点、`source.kind` が `none`、`test_oscillator` が `true` |
| 上書き | 確認の表示のあと `Overwrite` で置き換わり、番組時間 118.58 秒、履歴 1185 点になった |
| 計測の継続 | 書き出しの間も Time と Integrated が進み続けた |
| 日本語表示 | 見出しが「測定結果の書き出し」、確認が「ファイルが存在します。「…」を置き換えますか？」になった |

音声ファイルを入力にしたときのフルパスの扱いは、この確認では測っていません。
単体テストで、既定では名前だけになり、指定したときだけフルパスが入ることを確認しています。
