# ビルド方法

開発環境の準備、ビルド、テスト、CI の仕組みを説明します。

[ドキュメント一覧に戻る](README.md) ・ [README](../README.md)

## 必要なもの

| 項目 | 内容 |
|---|---|
| Rust | stable（`rust-version` は 1.85 以上） |
| Windows | Visual Studio 2022 Build Tools（C++ によるデスクトップ開発） |
| macOS | Xcode Command Line Tools |
| Linux | 下記の開発パッケージ |

Debian / Ubuntu では、次のコマンドでビルドに必要なパッケージを入れます。

```sh
sudo apt-get install -y build-essential pkg-config libasound2-dev libgl-dev libjack-dev \
  libx11-xcb-dev libxcb1-dev libxcb-dri2-0-dev libxcb-icccm4-dev libxcursor-dev \
  libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## ビルド

次のコマンドで、VST3 と CLAP のバンドルを `target/bundled/` に作成します。

```sh
cargo xtask bundle shirabe-plugin --release
```

次のコマンドで、スタンドアロンアプリを `target/release/shirabe`（Windows では `shirabe.exe`）に作成します。

```sh
cargo build --release -p shirabe-plugin --bin shirabe
```

## テスト

次のコマンドで、ワークスペース全体のテストを実行します。

```sh
cargo test --workspace
```

次のコマンドで、CI と同じ書式チェックと静的解析を実行します。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## スタンドアロンアプリでの動作確認

デバッグビルドでは、オーディオスレッドでメモリ確保が起きると強制終了します。

次のコマンドで、音声デバイスなしでデバッグビルドを起動します。

```sh
cargo run -p shirabe-plugin --bin shirabe -- --backend dummy
```

起動後に「Test oscillator」をオンにすると、メーターが動くことを確認できます。

## CI

`.github/workflows/ci.yml` で次の処理を行います。

| ジョブ | 実行条件 | 内容 |
|---|---|---|
| `check` | push、pull request | `cargo fmt`、`cargo clippy`、`cargo test` |
| `bundle (windows-x64)` | `check` の成功後 | VST3 / CLAP / スタンドアロンのビルド、pluginval による VST3 検証 |
| `bundle (linux-x64)` | `check` の成功後 | VST3 / CLAP / スタンドアロンのビルド、pluginval による VST3 検証 |
| `bundle (macos-arm64)` | タグの push、または手動実行で `macos` を指定したとき | VST3 / CLAP / スタンドアロンのビルド |

macOS のランナーは非公開リポジトリでは消費時間が多いため、通常の push では実行しません。

手動で macOS を含めてビルドするには、次のコマンドを実行します。

```sh
gh workflow run CI -f macos=true
```

## 依存関係の固定

`nih_plug` は crates.io に公開されていないため、ワークスペースの `Cargo.toml` で Git のコミットを固定しています。

更新するときは `rev` を変更し、CI と実機での確認を行ってからマージします。
