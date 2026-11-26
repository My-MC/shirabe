# 配布パッケージ

署名なしの配布パッケージを作るスクリプトです。
置き場所と使い方は [docs/building.md](../docs/building.md)、確認方法は [docs/testing.md](../docs/testing.md) に書いています。

| パス | 内容 |
|---|---|
| `version.sh` | `Cargo.toml` の `[workspace.package] version` を表示します |
| `windows/shirabe.iss` | Inno Setup 6 の定義です |
| `windows/build-installer.ps1` | インストーラーを作り、Visual C++ ランタイムへの依存を確認します |
| `windows/test-installer.ps1` | サイレントインストールと削除を確認します（使い捨ての環境専用） |
| `linux/build-tarball.sh` | 再現可能な tar.gz を作ります |
| `linux/install.sh` | ユーザー向け、または `--system` でインストールします |
| `linux/uninstall.sh` | 同じ場所から削除します |
| `linux/smoke-test.sh` | 一時ディレクトリだけでインストールと削除を確認します |
| `linux/shirabe.desktop`、`linux/shirabe.svg` | アプリケーションメニューの項目とアイコンです |

## 版

版は `Cargo.toml` の `[workspace.package] version` だけを出所にします。
インストーラーのファイル名と、tar.gz のディレクトリ名に使います。

## AppId

Windows インストーラーの AppId は `{FDE72B4E-A801-4085-89A8-50C9A6CF8B63}` です。
AppId はアップグレードとアンインストールの登録に使う製品の同一性です。
一度公開したら変更しません。
変更すると Windows は別の製品として扱い、古い登録とファイルが残ります。

## 作る

```sh
packaging/linux/build-tarball.sh --dist dist/shirabe-linux-x64
packaging/linux/smoke-test.sh dist/installers/*.tar.gz
```

```powershell
.\packaging\windows\build-installer.ps1
```

Linux の tar.gz は、同じ入力と同じ `SOURCE_DATE_EPOCH` ならバイト単位で同じファイルになります。
Windows のインストーラーは Inno Setup の圧縮と PE のタイムスタンプのため、同じ入力でもバイト単位では一致しません。
