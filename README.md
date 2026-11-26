# Shirabe Meter Suite

Shirabe（調べ）は、放送や音楽制作で使われる各種メーターを、1つの画面で切り替えたり組み合わせたりして表示するためのオーディオメーターです。

VST3 / CLAP プラグインと、DAW なしで動くスタンドアロンアプリの両方として提供します。

> [!NOTE]
> 現在はフェーズ 1（規格に沿った基本メーター）の開発中です。
> サンプルピーク、トゥルーピーク、VU、PPM、ラウドネスを表示できます。リサージュなどのステレオ解析はフェーズ 2 で追加します。
> 計画は [docs/roadmap.md](docs/roadmap.md)、各メーターが準拠する規格は [docs/standards.md](docs/standards.md) を参照してください。

## 特長（目標）

- 音を一切加工しないパススルー動作で、レイテンシーは 0 です。
- VU、PPM、トゥルーピーク、ラウドネス（ITU-R BS.1770 / EBU R128 / ARIB TR-B32）、リサージュ、相関、ベクタースコープ、スペクトラムを順次実装します。
- サラウンド（5.1 / 7.1）への対応は将来の予定です。
- 画面をタイル状に分割し、各タイルに好きなメーターを割り当てられるようにします。
- レイアウトとテーマはファイルとして保存・共有できるようにします。
- Windows x64 を最優先とし、macOS と Linux にも対応します。

## 現在できること

| 機能 | 内容 |
|---|---|
| サンプルピークメーター | モノラル / ステレオの各チャンネル、ピークホールド、リセット可能な最大値表示 |
| トゥルーピーク | 4 倍オーバーサンプリング（ITU-R BS.1770 Annex 2 を参照）、チャンネルごとの最大値 |
| VU / PPM | VU（バー／針の切り替え、電圧比例の目盛りと % 目盛り、0 VU の基準レベルを変更可能）と PPM（Type I、EBU 目盛り、BBC 目盛り、IEEE 目盛り）を切り替えて表示 |
| ラウドネス | Momentary、Short-term、Integrated、LRA と各最大値。EBU +9 / +18 目盛り、LUFS / LU 表示、I と LRA の一時停止（ITU-R BS.1770-5、EBU Tech 3341 / 3342。EBU の公式テスト音源の全ケースで確認） |
| 目標値プリセット | ARIB TR-B32、EBU R 128、ATSC A/85、Spotify（-14 LUFS）、Apple Podcasts（-16 LKFS）、Custom（目標値・許容範囲・トゥルーピークを指定） |
| リセット | ボタン、メーター部のダブルクリック、ホストの再生開始（オン / オフ可能） |
| テストオシレーター | 入力をサイン波に置き換えて、メーターの校正や動作確認に使えます（初期状態はオフ） |
| 出力形式 | VST3、CLAP、スタンドアロンアプリ |

## ダウンロード

GitHub Actions の各実行結果の Artifacts から、プラットフォームごとのビルドを入手できます。

| Artifact | 中身 |
|---|---|
| `shirabe-windows-x64` | `Shirabe Meter Suite.vst3`、`Shirabe Meter Suite.clap`、`shirabe.exe` |
| `shirabe-linux-x64` | `Shirabe Meter Suite.vst3`、`Shirabe Meter Suite.clap`、`shirabe` |
| `shirabe-macos-arm64` | `Shirabe Meter Suite.vst3`、`Shirabe Meter Suite.clap`、`shirabe`（署名なし） |

## インストール（Windows）

1. `Shirabe Meter Suite.vst3` フォルダーを `C:\Program Files\Common Files\VST3\` にコピーします。
2. CLAP を使う場合は、`Shirabe Meter Suite.clap` を `C:\Program Files\Common Files\CLAP\` にコピーします。
3. ホストアプリケーションでプラグインを再スキャンします。

スタンドアロンアプリは `shirabe.exe` をそのまま起動します。

## スタンドアロンアプリの使い方

スタンドアロンアプリは、入力デバイスの音か、コンピューターで再生中の音を測定します。
画面上部の「Audio API」で WASAPI や ALSA などのオーディオ API を、「Source」で測定する音を選びます。

| Source の項目 | 測定する音 |
|---|---|
| Inputs の各デバイス | マイクやオーディオインターフェースの入力 |
| Playing on this computer の各項目（名前の後ろに `(playing)`） | そのコンピューターで再生中の音 |
| None | 無音（テストオシレーターでの動作確認用） |

再生中の音の取り込み方は OS ごとに異なります。

| OS | 方法 | 注意点 |
|---|---|---|
| Windows | 出力デバイスの WASAPI ループバック（Audio API で WASAPI を選んだときに表示） | 再生ソフトが WASAPI 排他モードや ASIO で出力していると取り込めません。共有モードで再生してください |
| Linux | PulseAudio / PipeWire の出力のモニター | PulseAudio または PipeWire（pipewire-pulse）が動いている必要があります |
| macOS 14.2 以降 | Core Audio のプロセスタップ（システム全体の音） | 初回に「システムオーディオ録音」の許可を求められます。許可しないと無音になります |

いずれの方法でも、測定するのは OS のミキサーを通った後の音です。
システム音など他のアプリの音も含まれ、サンプルレートは出力デバイスの設定に変換された後の値になります。
再生ソフトの音をそのまま測りたいときは、再生ソフトに VST3 / CLAP 版を挿してください。

- サンプルレート、チャンネル数、サンプル形式は、デバイスの設定をそのまま使います。
- 3 チャンネル以上のデバイスでは、チャンネル 1 と 2 を測定します。
- 音が届かない間（Windows のループバックは再生が止まると何も送りません）は、無音として測定を続けます。
- 選んだ Source と各設定は、次回の起動時にも引き継がれます。

設定ファイルの場所は次のとおりです。

| OS | 場所 |
|---|---|
| Windows | `%APPDATA%\Shirabe\standalone.json` |
| macOS | `~/Library/Application Support/Shirabe/standalone.json` |
| Linux | `$XDG_CONFIG_HOME/shirabe/standalone.json`（既定は `~/.config/shirabe/`） |

音声ファイルを再生しながらの測定は、[ロードマップ](docs/roadmap.md) のフェーズ 1 で追加します。

## ドキュメント

- [ドキュメント一覧](docs/README.md)
- [設計](docs/design.md)
- [ロードマップ](docs/roadmap.md)
- [ビルド方法](docs/building.md)
- [変更履歴](CHANGELOG.md)

## ライセンス

| 対象 | ライセンス |
|---|---|
| `crates/shirabe-dsp`（計測ライブラリ） | MIT または Apache-2.0 のいずれかを選択 |
| 上記以外（プラグイン本体、UI、スタンドアロンアプリ） | GPL-3.0-or-later |

ライセンス本文は [`LICENSES/`](LICENSES/) にあります。

このプラグインを使って制作した音声や楽曲には、GPL は適用されません。

VST is a registered trademark of Steinberg Media Technologies GmbH.

## English summary

Shirabe Meter Suite is a pass-through, zero-latency audio metering suite delivered as VST3 and CLAP plug-ins and a standalone application.

It is in early development: only a sample peak meter is implemented so far.

The DSP crate `shirabe-dsp` is MIT OR Apache-2.0; everything else is GPL-3.0-or-later.

Audio you produce with it is not affected by the GPL.
