# Shirabe Meter Suite

Shirabe（調べ）は、放送や音楽制作で使われる各種メーターを、1つの画面で切り替えたり組み合わせたりして表示するためのオーディオメーターです。

VST3 / CLAP プラグインと、DAW なしで動くスタンドアロンアプリの両方として提供します。

> [!NOTE]
> 現在はフェーズ 0（基盤づくり）の段階です。
> 今あるメーターはサンプルピークのみで、ラウドネスやリサージュなどは順次追加します。
> 計画は [docs/roadmap.md](docs/roadmap.md) を参照してください。

## 特長（目標）

- 音を一切加工しないパススルー動作で、レイテンシーは 0 です。
- VU、PPM、トゥルーピーク、ラウドネス（ITU-R BS.1770 / EBU R128 / ARIB TR-B32）、リサージュ、相関、ベクタースコープ、サラウンド、スペクトラムを順次実装します。
- 画面をタイル状に分割し、各タイルに好きなメーターを割り当てられるようにします。
- レイアウトとテーマはファイルとして保存・共有できるようにします。
- Windows x64 を最優先とし、macOS と Linux にも対応します。

## 現在できること

| 機能 | 内容 |
|---|---|
| サンプルピークメーター | モノラル / ステレオ / 5.1 の各チャンネル、ピークホールド、リセット可能な最大値表示 |
| テストオシレーター | 入力をサイン波に置き換えて、メーターの校正や動作確認に使えます（初期状態はオフ） |
| 出力形式 | VST3、CLAP、スタンドアロンアプリ |

## ダウンロード

GitHub Actions の各実行結果の Artifacts から、プラットフォームごとのビルドを入手できます。

| Artifact | 中身 |
|---|---|
| `shirabe-windows-x64` | `Shirabe Meter Suite.vst3`、`Shirabe Meter Suite.clap`、`shirabe.exe` |
| `shirabe-linux-x64` | `Shirabe Meter Suite.vst3`、`Shirabe Meter Suite.clap`、`shirabe` |
| `shirabe-macos-arm64` | タグ付きビルドまたは手動実行時のみ作成されます |

## インストール（Windows）

1. `Shirabe Meter Suite.vst3` フォルダーを `C:\Program Files\Common Files\VST3\` にコピーします。
2. CLAP を使う場合は、`Shirabe Meter Suite.clap` を `C:\Program Files\Common Files\CLAP\` にコピーします。
3. ホストアプリケーションでプラグインを再スキャンします。

スタンドアロンアプリは `shirabe.exe` をそのまま起動します。

## スタンドアロンアプリの使い方

オーディオデバイスやバックエンドはコマンドラインで指定します。

次のコマンドで、使えるオプションの一覧を表示します。

```sh
shirabe --help
```

次のコマンドは、音声デバイスなしで画面だけを起動します（テストオシレーターで動作確認できます）。

```sh
shirabe --backend dummy
```

デバイスを画面から選べるスタンドアロン専用の UI は、[ロードマップ](docs/roadmap.md) のフェーズ 1 以降で追加します。

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
