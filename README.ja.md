# lnch

> **One YAML. One command. All your services.**

開発環境のマルチプロセスランチャー TUI — ローカルサーバーやコマンドをひとつのターミナルで一括管理。

[English](README.md) | **日本語**

```
┌─ lnch: my-project ─────────────────────────────────────────────────┐
│                                                                     │
│  Tasks              │  Logs: [frontend]                             │
│  ─────              │  ───────────────────────────────────────────  │
│  ● frontend  [3000] │  ▶ ready - started server on 0.0.0.0:3000    │
│  ● backend   [8080] │  ▶ compiled client and server successfully    │
│  ● database         │  ▶ watching for file changes...               │
│  ○ worker    [stop] │                                               │
│                     │                                               │
├─────────────────────┴───────────────────────────────────────────────┤
│ [a] All Start  [s] Start/Stop  [r] Restart  [↑↓] Select  [q] Quit  │
└─────────────────────────────────────────────────────────────────────┘
```

## 特徴

- **1つのYAML、1つのコマンド** — `lnch.yaml` にサービスを定義し、`lnch` で一括起動
- **TUI ダッシュボード** — [ratatui](https://github.com/ratatui/ratatui) による分割ペインでプロセスの状態とログをリアルタイム監視
- **依存順序制御** — `depends_on` でサービスの起動順序をトポロジカルソートにより自動解決
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

### Homebrew (macOS / Linux)

```bash
brew install shell-term/tap/lnch
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

### 設定ファイルの探索

`--config` を指定せずに実行した場合、`lnch` はカレントディレクトリから上位に向かって `lnch.yaml` を探索します（最大10階層）。プロジェクトのどのサブディレクトリからでも `lnch` を実行できます。

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
