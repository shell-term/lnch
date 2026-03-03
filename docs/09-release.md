# リリース手順

> **最終更新**: 2026-03-03

このドキュメントでは、lnch の新バージョンをリリースしてバイナリ配布するまでの手順を説明する。

---

## 前提

- ブランチモデル: `develop`（開発）→ `main`（リリース）
- リリース自動化: [cargo-dist](https://axodotdev.github.io/cargo-dist/) v0.31.0
- CI: GitHub Actions（`.github/workflows/release.yml`、dist が自動生成）
- トリガー: `v*.*.*` 形式の Git タグ push

## 配布されるもの

| アーティファクト | 対象 |
|-----------------|------|
| `lnch-x86_64-apple-darwin.tar.xz` | macOS (Intel) |
| `lnch-aarch64-apple-darwin.tar.xz` | macOS (Apple Silicon) |
| `lnch-x86_64-unknown-linux-gnu.tar.xz` | Linux (x64) |
| `lnch-aarch64-unknown-linux-gnu.tar.xz` | Linux (ARM64) |
| `lnch-x86_64-pc-windows-msvc.zip` | Windows (x64) |
| `lnch-installer.sh` | macOS / Linux ワンライナーインストーラー |
| `lnch-installer.ps1` | Windows PowerShell インストーラー |

---

## リリース手順

### 1. develop ブランチで準備

リリース対象の機能・修正がすべて `develop` にマージされていることを確認する。

```bash
git checkout develop
git pull origin develop
```

### 2. バージョン番号を更新

`Cargo.toml` の `version` フィールドをリリースバージョンに変更する。

```toml
[package]
version = "0.2.0"   # ← 新バージョンに変更
```

### 3. RELEASES.md を更新

`RELEASES.md` の先頭に新バージョンのセクションを追加する。cargo-dist がこのファイルから GitHub Release のリリースノートを自動抽出する。

```markdown
# v0.2.0

- 新機能Aを追加
- バグBを修正
- ...

# v0.1.0

- Initial release
...
```

### 4. リリース準備コミット

```bash
git add Cargo.toml Cargo.lock RELEASES.md
git commit -m "chore: bump version to 0.2.0"
git push origin develop
```

### 5. develop → main にマージ

GitHub 上で Pull Request を作成してマージする、またはローカルでマージする。

```bash
git checkout main
git pull origin main
git merge develop
git push origin main
```

### 6. タグを作成して push

タグ push が GitHub Actions のリリースワークフローを起動するトリガーとなる。

```bash
git tag "v0.2.0"
git push origin "v0.2.0"
```

### 7. CI の完了を待つ

GitHub の Actions タブでリリースワークフローの進行を確認する。正常に完了すると:

1. 全 5 プラットフォームのバイナリがビルドされる
2. GitHub Release が自動作成される
3. インストーラースクリプトがアップロードされる
4. RELEASES.md から抽出されたリリースノートが Release の本文に設定される

### 8. GitHub Release を確認

リポジトリの Releases ページで以下を確認する:

- リリースノートが正しく表示されている
- 全アーティファクトがアップロードされている
- インストーラーの URL が正しい

### 9. develop のバージョンを次の開発バージョンに進める（任意）

```bash
git checkout develop
git merge main
git push origin develop
```

---

## ユーザー向けインストールコマンド

リリース後、ユーザーは以下のいずれかでインストールできる。

**macOS / Linux:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -c "irm https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.ps1 | iex"
```

**cargo-binstall:**

```bash
cargo binstall lnch
```

---

## cargo-dist 設定の変更

インストーラーやターゲットプラットフォームを変更したい場合は `dist-workspace.toml` を編集し、`dist generate` で CI ワークフローを再生成する。

```bash
# dist-workspace.toml を編集した後
dist generate
```

`dist generate` は `.github/workflows/release.yml` を上書きするため、ワークフローを手動で編集しないこと。

---

## トラブルシューティング

### CI が起動しない

- タグが `v*.*.*` 形式であることを確認する（例: `v0.1.0`）
- タグがリリースワークフローの `.yml` を含むコミット以降に作成されていることを確認する

### 特定プラットフォームのビルドが失敗する

- GitHub Actions のログでエラーを確認する
- `dist plan` をローカルで実行して設定に問題がないか確認する
- `dist build` で現在のプラットフォーム向けにローカルビルドを試す

### リリースノートが表示されない

- `RELEASES.md` のバージョン見出しが `Cargo.toml` のバージョンと一致しているか確認する
- 見出し形式: `# v0.2.0`（`v` プレフィックスを付ける）
