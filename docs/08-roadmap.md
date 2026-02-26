# ロードマップ・開発計画

## 1. バージョン概要

| バージョン | 機能 | 概要 |
|-----------|------|------|
| **v0.1 (MVP)** | コア機能 | YAMLを手書き → `lnch` でTUI起動 → 一括管理 |
| **v0.2** | グローバルプロファイル | `~/.config/lnch/profiles.yaml` による複数プロジェクト横断管理 |
| **v0.3** | TUIからの設定編集 | YAMLを手で書かなくても、TUI上でタスクの追加・編集・削除が完結 |
| **v0.4** | テンプレート & init | `lnch init` で対話的にYAMLを生成。よく使う構成のテンプレート |
| **v0.5** | ヘルスチェック & 通知 | ポートの死活監視、プロセスクラッシュ時の自動再起動オプション |

---

## 2. v0.2 — グローバルプロファイル

**概要**: `~/.config/lnch/profiles.yaml` による複数プロジェクト横断管理

**主な変更点:**
- グローバル設定ファイルのスキーマ定義と読み込み
- `lnch -p <profile>` オプションの追加
- `lnch list` サブコマンドの追加（プロファイル一覧表示）
- ローカル設定とグローバル設定の優先順位ルール

**設定ファイル例:**

```yaml
# ~/.config/lnch/profiles.yaml
profiles:
  - name: ses-work
    tasks:
      - name: api
        command: cargo run
        working_dir: ~/projects/trade-ocr/backend
      - name: front
        command: npm run dev
        working_dir: ~/projects/trade-ocr/frontend

  - name: my-product
    tasks:
      - name: lnch-dev
        command: cargo run
        working_dir: ~/projects/lnch
```

**CLI拡張:**

```
lnch list                # プロファイル一覧
lnch -p <profile>        # 指定プロファイルでTUI起動
```

---

## 3. v0.3 — TUIからの設定編集

**概要**: TUI上でタスクの追加・編集・削除が完結。YAMLファイルへの書き戻し。

**主な変更点:**
- 設定編集モード用のTUI画面（フォーム入力ウィジェット）
- YAML書き戻しロジック（コメント保持はベストエフォート）
- `F2` キーで設定編集モードへ切り替え

**画面イメージ:**

```
┌─ lnch: Settings ──────────────────────────────────────────────────┐
│                                                                    │
│  Tasks              │  Edit Task: [frontend]                       │
│  ─────              │  ──────────────────────────────────────────  │
│  > frontend         │  Name:        [frontend          ]          │
│    backend          │  Command:     [npm run dev -- --port 3000 ]  │
│    database         │  Working Dir: [./frontend         ]          │
│    worker           │  Color:       [green ▼]                      │
│                     │                                              │
│  [+] Add Task       │  Env Variables:                              │
│                     │    NEXT_PUBLIC_API_URL = [http://localho...]  │
│                     │    [+] Add Variable                          │
│                     │                                              │
│                     │  Depends On:  [ ] database  [ ] backend      │
│                     │                                              │
│                     │  [Save]  [Cancel]  [Delete Task]             │
├─────────────────────┴──────────────────────────────────────────────┤
│ [Tab] Next Field  [Enter] Edit  [Esc] Cancel       [F2] Settings   │
└────────────────────────────────────────────────────────────────────┘
```

**設計方針:**
- YAML が常に Single Source of Truth
- TUI編集時も即座にYAMLファイルへ書き戻し
- 手書き編集とTUI編集のどちらも可能

---

## 4. v0.4 — テンプレート & init

**概要**: `lnch init` で対話的にYAMLを生成。よく使う構成のテンプレート。

**主な変更点:**
- `lnch init` サブコマンドの追加
- 対話型プロンプトでプロジェクト名・タスクを入力
- プリセットテンプレート（Next.js + API, Docker Compose, etc.）

**CLI拡張:**

```
lnch init                    # 対話的にlnch.yamlを生成
lnch init --template nextjs  # テンプレートから生成
```

---

## 5. v0.5 — ヘルスチェック & 自動再起動

**概要**: ポートの死活監視、プロセスクラッシュ時の自動再起動

**主な変更点:**
- タスクごとの `restart` ポリシー設定（`never` / `on_failure` / `always`）
- `health_check` 設定（HTTP / TCP ポート監視）
- 自動再起動のバックオフ戦略（指数バックオフ、最大再試行回数）

**設定ファイル拡張例:**

```yaml
tasks:
  - name: backend
    command: cargo run -- --port 8080
    restart: on_failure         # never | on_failure | always
    max_restarts: 5             # 最大再起動回数
    health_check:
      type: tcp                 # tcp | http
      port: 8080
      interval: 10s
      timeout: 5s
```

---

## 6. v0.1 MVP 開発スケジュール（3週間）

### Week 1: コアロジック

| # | タスク | 成果物 |
|---|--------|--------|
| 1 | Rustプロジェクトセットアップ（Cargo.toml、CI） | プロジェクト骨格 |
| 2 | 設定ファイルのデータモデル定義 | `config/model.rs` |
| 3 | YAML読み込み・ファイル探索 | `config/loader.rs` |
| 4 | バリデーション | `config/validator.rs` |
| 5 | 依存関係解決（トポロジカルソート） | `process/dependency.rs` |
| 6 | ログバッファ | `log/buffer.rs` |
| 7 | ユニットテスト | `tests/config_test.rs`, `tests/dependency_test.rs` |

### Week 2: TUI + プロセス管理

| # | タスク | 成果物 |
|---|--------|--------|
| 8 | TaskRunner（プロセス起動・停止・ログ収集） | `process/task_runner.rs` |
| 9 | ProcessManager（全タスク統括） | `process/manager.rs` |
| 10 | TUIレイアウト（タスクリスト + ログビュー + ステータスバー） | `tui/ui.rs`, `tui/widgets/` |
| 11 | イベントループとキーバインド | `tui/app.rs`, `tui/event.rs` |
| 12 | コアロジックとTUIの結合 | 動作するプロトタイプ |

### Week 3: 仕上げ

| # | タスク | 成果物 |
|---|--------|--------|
| 13 | シグナルハンドリング・クリーンアップ | `process/signal.rs` |
| 14 | プロセスグループによる確実なKill | プラットフォーム別テスト |
| 15 | `depends_on` の統合テスト | `tests/process_test.rs` |
| 16 | クロスプラットフォームビルド確認（Mac/Linux） | CI設定 |
| 17 | `cargo install` 対応 | `Cargo.toml` メタデータ |
| 18 | README作成・GitHubリポジトリ整備 | `README.md` |

---

## 7. マイルストーン

```
v0.1 (MVP)  ─── Week 3 完了時点でリリース
  │
  │  +2〜3週間
  ▼
v0.2 (グローバルプロファイル)
  │
  │  +2〜3週間
  ▼
v0.3 (TUI設定編集)
  │
  │  +2週間
  ▼
v0.4 (テンプレート & init)
  │
  │  +2〜3週間
  ▼
v0.5 (ヘルスチェック & 自動再起動)
```
