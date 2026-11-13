# 変更履歴

このプロジェクトの主な変更を記録します。

書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従い、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

### 追加

- Cargo ワークスペース（`shirabe-dsp`、`shirabe-ui`、`shirabe-plugin`、`xtask`）を作成しました。
- サンプルピークメーター（モノラル / ステレオ / 5.1、ピークホールド、最大値表示）を追加しました。
- 校正・動作確認用のテストオシレーターを追加しました。
- VST3、CLAP、スタンドアロンアプリのビルドに対応しました。
- GitHub Actions による fmt / clippy / テスト、Windows・Linux のバンドル作成、pluginval による VST3 検証を追加しました。
