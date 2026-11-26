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
cargo build --release -p shirabe-app --bin shirabe
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

## ホストでの動作確認

プラグインホストを使った確認と自動テストは [testing.md](testing.md) にまとめています。

## スタンドアロンアプリでの動作確認

デバッグビルドでは、オーディオスレッドでメモリ確保が起きると強制終了します。

次のコマンドで、デバッグビルドを起動します。

```sh
cargo run -p shirabe-app --bin shirabe
```

「Input」で「None」を選び「Test oscillator」をオンにすると、デバイスなしでメーターが動くことを確認できます。
入力デバイスでの確認手順は [testing.md](testing.md) にあります。

## CI

`.github/workflows/ci.yml` で次の処理を行います。

| ジョブ | 実行条件 | 内容 |
|---|---|---|
| `check` | push、pull request | `cargo fmt`、`cargo clippy`、`cargo test` |
| `bundle (windows-x64)` | `check` の成功後 | VST3 / CLAP / スタンドアロンのビルド、pluginval による VST3 検証、インストーラーの作成とサイレントインストールの確認 |
| `bundle (linux-x64)` | `check` の成功後 | VST3 / CLAP / スタンドアロンのビルド、pluginval による VST3 検証、tar.gz の作成と一時 HOME へのインストール確認 |
| `bundle (macos-arm64)` | `check` の成功後 | VST3 / CLAP / スタンドアロンのビルド、zip の作成 |

手動で CI を実行するには、次のコマンドを実行します。

```sh
gh workflow run CI
```

## 配布パッケージ

CI は毎回、バンドルとは別に、署名なしの配布パッケージを作成します。
リリースの公開は行わず、GitHub Actions の Artifacts にだけ残します。

| Artifact | 内容 |
|---|---|
| `shirabe-installer-windows-x64` | `ShirabeMeterSuite-<版>-windows-x64-setup.exe` |
| `shirabe-installer-linux-x64` | `ShirabeMeterSuite-<版>-linux-x64.tar.gz` |
| `shirabe-installer-macos-arm64` | `ShirabeMeterSuite-<版>-macos-arm64.zip` |

版は `Cargo.toml` の `[workspace.package] version` から取ります。
どのパッケージにも README、CHANGELOG、`LICENSES/` を含めます。

### Windows インストーラー

Inno Setup 6 で作成し、管理者としてインストールします。

| 内容 | 場所 |
|---|---|
| VST3 | `%CommonProgramFiles%\VST3\Shirabe Meter Suite.vst3` |
| CLAP | `%CommonProgramFiles%\CLAP\Shirabe Meter Suite.clap` |
| スタンドアロン | `%ProgramFiles%\Shirabe Meter Suite\shirabe.exe` |
| スタートメニュー | `Shirabe Meter Suite` フォルダー（アプリとアンインストーラー） |
| アンインストーラー | `%ProgramFiles%\Shirabe Meter Suite\unins000.exe` |
| ライセンス | `%ProgramFiles%\Shirabe Meter Suite\licenses\` |

アンインストールしても `%APPDATA%\Shirabe` の設定は残します。
プラグインを読み込んでいるホストがあると置き換えに失敗するため、インストール前にホストを終了してください。

Windows で作成するには、Inno Setup 6 を入れてから次を実行します。

```powershell
.\packaging\windows\build-installer.ps1
```

`ISCC.exe` が見つからないときは `-Iscc` で場所を指定します。
スクリプトは、VST3、CLAP、`shirabe.exe` が Visual C++ ランタイムを要求しないことも確認します。

インストーラーの AppId は `{FDE72B4E-A801-4085-89A8-50C9A6CF8B63}` です。
AppId はアップグレードとアンインストールの登録に使う製品の同一性なので、公開後は変更しません。
変更すると Windows は別の製品として扱い、古い登録とファイルが残ります。

署名は行いません。
そのため、初回の実行時に SmartScreen の警告が出ることがあります。
「詳細情報」から実行を選ぶ必要があります。

### Linux の tar.gz

`packaging/linux/build-tarball.sh` が次の内容の tar.gz を作ります。

| 内容 | 説明 |
|---|---|
| `Shirabe Meter Suite.vst3` | VST3 バンドル |
| `Shirabe Meter Suite.clap` | CLAP |
| `shirabe` | スタンドアロンアプリ |
| `install.sh`、`uninstall.sh` | インストールと削除 |
| `shirabe.desktop`、`shirabe.svg` | アプリケーションメニューの項目とアイコン |
| `README.md`、`CHANGELOG.md`、`LICENSES/` | ドキュメントとライセンス |

展開して `install.sh` を実行すると、現在のユーザー向けに `~/.vst3`、`~/.clap`、`~/.local/bin`、`${XDG_DATA_HOME:-~/.local/share}` へ入ります。

```sh
tar -xzf ShirabeMeterSuite-0.1.0-linux-x64.tar.gz
cd ShirabeMeterSuite-0.1.0-linux-x64
./install.sh
```

`sudo ./install.sh --system` は `/usr/local` の下（`lib/vst3`、`lib/clap`、`bin`、`share`）へ入れます。
`--prefix` を付けると別の場所へ入れられ、書き込める場所なら root は不要です。
削除は同じオプションを付けた `uninstall.sh` で行います。
設定（`~/.config/shirabe`）と、同じディレクトリにある他のプラグインは消しません。

作成は再現可能です。
ファイルの時刻、所有者、順序と gzip のヘッダーを `SOURCE_DATE_EPOCH`（既定は最後のコミット時刻）に合わせるため、同じ入力と同じ値ならバイト単位で同じファイルになります。

```sh
packaging/linux/build-tarball.sh --dist dist/shirabe-linux-x64
packaging/linux/smoke-test.sh dist/installers/*.tar.gz
```

`smoke-test.sh` は一時ディレクトリの HOME と `--prefix` にインストールし、ファイルを確認してから削除します。
実際の `~/.vst3` などには触れません。

### macOS

macOS はインストーラーを作らず、3 つのバンドルを `ShirabeMeterSuite-<版>-macos-arm64.zip` にまとめます。
署名と公証は行いません（保留）。
そのため、初回は Gatekeeper が実行を止めることがあります。
Finder で右クリックして「開く」を選ぶか、展開したディレクトリの隔離属性を外します。

```sh
xattr -dr com.apple.quarantine ShirabeMeterSuite-0.1.0-macos-arm64
```

ホストアプリケーションが署名のないプラグインを拒否する場合があります。
macOS での動作はビルドの成功までしか確認していません（[testing.md](testing.md)）。

## 依存関係の固定

`nih_plug` は crates.io に公開されていないため、ワークスペースの `Cargo.toml` で Git のコミットを固定しています。

次の 3 つは、修正を加えたフォークを使っています。いずれも `shirabe` ブランチに修正をまとめています。

| フォーク | 元のリポジトリ | 主な修正 |
|---|---|---|
| [My-MC/nih-plug](https://github.com/My-MC/nih-plug/tree/shirabe) | robbert-vdh/nih-plug | CLAP の状態読み込みの安全化と、読み込み後のパラメーター再取得の通知（[#3](https://github.com/My-MC/shirabe/issues/3)） |
| [My-MC/baseview](https://github.com/My-MC/baseview/tree/shirabe) | RustAudio/baseview | OpenGL のコンテキストを作れないときに panic せず、sRGB なしでも再試行する（[#1](https://github.com/My-MC/shirabe/issues/1)） |
| [My-MC/egui-baseview](https://github.com/My-MC/egui-baseview/tree/shirabe) | BillyDM/egui-baseview | 描画の初期化に失敗してもウィンドウを開いたままにする（[#1](https://github.com/My-MC/shirabe/issues/1)） |

`baseview` と `egui-baseview` のフォークは、`nih_plug_egui` の依存先として `My-MC/nih-plug` 側で指定しています。

更新するときはフォーク側で修正してコミットし、`Cargo.toml` の `rev` を変更して、CI と実機での確認を行ってからマージします。
