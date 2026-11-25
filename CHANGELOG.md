# 変更履歴

このプロジェクトの主な変更を記録します。

書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従い、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

### 追加

- Cargo ワークスペース（`shirabe-dsp`、`shirabe-ui`、`shirabe-plugin`、`xtask`）を作成しました。
- サンプルピークメーター（モノラル / ステレオ、ピークホールド、最大値表示）を追加しました。
- 校正・動作確認用のテストオシレーターを追加しました。
- VST3、CLAP、スタンドアロンアプリのビルドに対応しました。
- GitHub Actions による fmt / clippy / テスト、Windows・Linux・macOS のバンドル作成、pluginval による VST3 検証を追加しました。

- Linux 上のホスト（Ardour、clap-validator、pluginval）で動作を確認する `tools/host-tests/run.sh` を追加しました。

### 修正

- Windows で Visual C++ 再頒布可能パッケージが入っていない環境では、スタンドアロンアプリが `VCRUNTIME140.dll` が見つからずに起動しなかった問題を修正しました。C ランタイムを静的リンクし、CI で依存がないことを確認します。
