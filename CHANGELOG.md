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
- トゥルーピークメーター（4 倍オーバーサンプリング）を追加しました。
- ラウドネスメーター（Momentary、Short-term、Integrated、LRA、各最大値）と、目標値プリセット（ARIB TR-B32、EBU R128、ATSC A/85、配信向け）を追加しました。
- VU メーターと PPM（Type I / Type II）を追加し、切り替えて表示できるようにしました。
- ホストの再生開始に合わせて、Integrated と最大値をリセットする設定を追加しました。
- ラウドネスの表示を EBU Tech 3341 に合わせました（EBU +9 / +18 目盛り、LUFS / LU 表示、全数値への単位表示、I と LRA の一時停止・再開、測定開始から 60 秒間の LRA の安定表示）。
- PPM に EBU 目盛り（EBU Tech 3205）と BBC 目盛りを追加しました。
- PPM に IEEE 目盛り（IEEE Std 152-1991 の推奨目盛り、-20〜+4 dB、TEST は -8 の位置）を追加しました。
- 各メーターが準拠する規格と対応状況をまとめた `docs/standards.md` を追加しました。
- IEEE Std 152-1991 の PPM と VU の要件を確認するテストを追加しました。
- EBU の公式テスト音源で Tech 3341 / 3342 の全ケースを確認するテストを追加しました（音源は各自で入手）。

### 修正

- PPM と VU メーターが、サンプルレートを割り切る高い周波数（48 kHz での 12 kHz や 16 kHz など）を最大 1 dB 以上低く表示していた問題を修正しました。オーバーサンプリングした信号を整流します。
- OpenGL 3.2 を使えない環境でエディターを開くと、ホストごと終了していた問題を修正しました。ウィンドウは開いたまま描画しない動作になります。sRGB 対応のフレームバッファがない環境では、sRGB なしで開き直して表示します（[#1](https://github.com/My-MC/shirabe/issues/1)）。
- CLAP 版で、壊れた状態データを読み込むと異常終了していた問題と、状態の読み込み後にホストへパラメーターの再取得を通知していなかった問題を修正しました（[#3](https://github.com/My-MC/shirabe/issues/3)）。

- 非正規化数の入力で処理が遅くなる問題を、処理中だけ FTZ / DAZ を有効にして解消しました。
- 値が 0 付近のときに `-0.0` と表示されないようにしました。
- Max M が、短い音の位置によって最大 0.2 LU ほど低く表示されていた問題を修正しました（EBU Tech 3341 ケース 13 / 14）。M と S の窓を 2.5 ms 以下の刻みで動かします。
- PPM Type II の応答を EBU Tech 3205 表 2 の値（10 ms で -2 dB）に合わせました。
- VU の目盛りを、電圧に比例する本来の配置（0 VU が全目盛りの約 71 %）に直しました。
- EBU R 128 の許容範囲の説明と、配信向けプリセット（Spotify の公表値に基づく）を資料に合わせて修正しました。

- Windows で Visual C++ 再頒布可能パッケージが入っていない環境では、スタンドアロンアプリが `VCRUNTIME140.dll` が見つからずに起動しなかった問題を修正しました。C ランタイムを静的リンクし、CI で依存がないことを確認します。
