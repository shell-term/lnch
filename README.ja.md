<div align="center">

# lnch

**One YAML. One command. All your services.**

[![Crates.io](https://img.shields.io/crates/v/lnch)](https://crates.io/crates/lnch)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](https://github.com/shell-term/lnch/releases)

開発環境のマルチプロセスランチャー TUI — ローカルサーバーやコマンドをひとつのターミナルで一括管理。

![demo](assets/lnch.gif)

</div>

[English](README.md) | **日本語**

## 特徴

- **1つのYAML、1つのコマンド** — `lnch.yaml` にサービスを定義し、`lnch` で一括起動
- **TUI ダッシュボード** — [ratatui](https://github.com/ratatui/ratatui) による分割ペインでプロセスの状態とログをリアルタイム監視
- **依存順序制御** — `depends_on` でサービスの起動順序をトポロジカルソートにより自動解決。`ready_check` で依存先の準備完了を待ってから起動
- **設定ファイル自動検出** — カレントディレクトリから上位に向かって `lnch.yaml` を探索するため、サブディレクトリからでも実行可能
- **タスクごとのログ** — stdout/stderr をリングバッファに保持し、色分け表示
- **グレースフルシャットダウン** — SIGTERM → タイムアウト後に SIGKILL。プロセスグループにより孤児プロセスを防止
- **クロスプラットフォーム** — macOS / Linux（WSL含む）/ Windows 対応

## インストール

### macOS / Linux

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.sh | sh
```

### Windows (PowerShell)

```powershell
powershell -c "irm https://github.com/shell-term/lnch/releases/latest/download/lnch-installer.ps1 | iex"
```

### cargo-binstall

```bash
cargo binstall lnch
```

### ソースからビルド（[Rust ツールチェーン](https://rustup.rs/)が必要）

```bash
cargo install --path .
```

### ローカルビルド

```bash
git clone https://github.com/shell-term/lnch.git
cd lnch
cargo build --release
# バイナリは ./target/release/lnch に出力されます
```

## クイックスタート

**1. プロジェクトルートに `lnch.yaml` を作成：**

```yaml
name: my-project

tasks:
  - name: frontend
    command: npm run dev
    working_dir: ./frontend
    env:
      PORT: "3000"
    color: green

  - name: backend
    command: cargo run -- --port 8080
    working_dir: ./backend
    color: blue
    depends_on:
      - database

  - name: database
    command: docker compose up postgres
    color: magenta
```

**2. 起動：**

```bash
lnch
```

これだけです。3つのサービスが依存順序に従って起動し、TUI で監視・制御できます。

## 使い方

```
lnch                     # lnch.yaml を自動検出して TUI を起動
lnch --config <path>     # 指定した設定ファイルで TUI を起動
lnch --version           # バージョン表示
lnch --help              # ヘルプ表示
```

### キーバインド

| キー | アクション |
|------|-----------|
| `↑` / `k` | 前のタスクを選択 |
| `↓` / `j` | 次のタスクを選択 |
| `a` | 全タスク起動 |
| `s` | 選択タスクの起動/停止トグル |
| `r` | 選択タスクの再起動 |
| `PageUp` | ログを上にスクロール |
| `PageDown` | ログを下にスクロール |
| `Home` | ログの先頭へ |
| `End` | ログの末尾へ（自動スクロール再開） |
| `c` | 選択タスクのログをクリア |
| `q` / `Ctrl+C` | 終了（グレースフルシャットダウン） |

## 設定ファイル

### `lnch.yaml` スキーマ

| フィールド | 型 | 必須 | デフォルト | 説明 |
|-----------|-----|------|-----------|------|
| `name` | `string` | はい | — | プロジェクト名（TUI タイトルバーに表示） |
| `tasks` | `list` | はい | — | タスク定義のリスト（1つ以上必須） |
| `tasks[].name` | `string` | はい | — | タスク名（リスト内で一意であること） |
| `tasks[].command` | `string` | はい | — | 実行するシェルコマンド |
| `tasks[].working_dir` | `string` | いいえ | 設定ファイルのディレクトリ | 作業ディレクトリ（`lnch.yaml` からの相対パスまたは絶対パス） |
| `tasks[].env` | `map` | いいえ | `{}` | 環境変数（親プロセスの環境変数を継承し、上書き） |
| `tasks[].color` | `string` | いいえ | 自動割当 | タスクの色: `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` |
| `tasks[].depends_on` | `list` | いいえ | `[]` | 先に起動すべきタスク名のリスト |
| `tasks[].ready_check` | `object` | いいえ | スマートデフォルト | 依存関係の準備完了チェック（後述） |

### 設定ファイルの探索

`--config` を指定せずに実行した場合、`lnch` はカレントディレクトリから上位に向かって `lnch.yaml` を探索します（最大10階層）。プロジェクトのどのサブディレクトリからでも `lnch` を実行できます。

### レディネスチェック

`depends_on` を指定したタスクは、依存先が「準備完了」になるまで起動を待機します。デフォルトではスマート判定（一発タスクは正常終了で ready、常駐タスクは2秒のグレースピリオド経過で ready）を使用します。より厳密な制御が必要な場合は `ready_check` を指定できます：

```yaml
tasks:
  - name: database
    command: docker compose up postgres
    ready_check:
      tcp: { port: 5432 }     # TCP ポートへの接続を待つ
      timeout: 60              # タイムアウト秒数（デフォルト: 30）

  - name: migrate
    command: sqlx migrate run
    ready_check:
      exit: {}                 # プロセスの正常終了（exit code 0）を待つ

  - name: backend
    command: cargo run
    depends_on: [database, migrate]
    ready_check:
      log_line: { pattern: "listening on port" }  # ログ出力のパターンマッチを待つ

  - name: frontend
    command: npm run dev
    depends_on: [backend]
    # ready_check 未指定: スマートデフォルト（常駐プロセスのグレースピリオド）
```

| チェック種類 | 説明 |
|-------------|------|
| `tcp: { port: <N> }` | `127.0.0.1:<port>` へのTCP接続が成功するまで待機 |
| `http: { url: "<URL>", status: <N> }` | HTTP GET で期待するステータスコード（デフォルト: 200）が返るまで待機。HTTPのみ対応 |
| `log_line: { pattern: "<text>" }` | stdout/stderr に指定文字列が含まれるまで待機 |
| `exit: {}` | プロセスがexit code 0で終了するまで待機 |

共通オプション: `timeout`（秒、デフォルト: 30）、`interval`（ミリ秒、デフォルト: 500）

タイムアウト時は警告ログを出力し、次のグループの起動を続行します。

### コマンドの実行方式

コマンドはOSのシステムシェル経由で実行されます：

| OS | シェル |
|----|--------|
| macOS / Linux | `sh -c "<command>"` |
| Windows | `cmd /C "<command>"` |

パイプ (`|`)、リダイレクト (`>`)、環境変数展開 (`$VAR`) などのシェル構文がそのまま利用できます。

## アーキテクチャ

```
lnch
├── CLI (clap)          — 引数解析、設定ファイルパスの解決
├── Config              — YAML読み込み、バリデーション、依存関係解決
├── Process Manager     — tokio チャネルによる非同期タスク管理
│   └── Task Runners    — 個別プロセスのライフサイクル（起動・IO・シグナル）
└── TUI (ratatui)       — 分割ペインUI、イベントループ
```

コンポーネント間は `tokio::mpsc` チャネルで通信します：
- **App → ProcessManager**: `ProcessCommand`（Start, Stop, Restart, Shutdown）
- **ProcessManager → App**: `ProcessEvent`（StatusChanged, LogLine, ProcessExited）

詳細な設計ドキュメントは [`docs/`](docs/) を参照してください。

## mprocs との比較

| 機能 | mprocs | lnch |
|------|--------|------|
| TUI でのプロセス管理 | ✅ | ✅ |
| YAML 設定ファイル | ✅ | ✅ |
| 設定ファイル自動検出（上位探索） | ❌ | ✅ |
| `depends_on` 起動順序制御 | ❌ | ✅ |
| グローバルプロファイル（複数PJ横断） | ❌ | ✅ (v0.2) |
| TUI からの設定編集 | ❌ | ✅ (v0.3) |
| `init` コマンドでテンプレート生成 | ❌ | ✅ (v0.4) |
| ヘルスチェック & 自動再起動 | ❌ | ✅ (v0.5) |

## ロードマップ

| バージョン | 内容 | ステータス |
|-----------|------|-----------|
| **v0.1** | MVP — YAML設定、TUI、プロセス管理、`depends_on` | 🚧 開発中 |
| **v0.2** | グローバルプロファイル (`~/.config/lnch/profiles.yaml`) | 計画中 |
| **v0.3** | TUI からの設定編集 | 計画中 |
| **v0.4** | `lnch init` 対話型プロンプト & テンプレート | 計画中 |
| **v0.5** | ヘルスチェック & 自動再起動ポリシー | 計画中 |

## コントリビューション

Issue や Pull Request を歓迎します。

```bash
# プロジェクトの実行
cargo run

# 設定ファイルを指定して実行
cargo run -- --config path/to/lnch.yaml

# テスト
cargo test

# フォーマット & リントチェック
cargo fmt --check
cargo clippy
```

## ライセンス

[MIT](LICENSE) © 2026 shell-term
