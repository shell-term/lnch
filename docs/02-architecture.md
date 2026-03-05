# システムアーキテクチャ

## 全体構成図

```
┌──────────────────────────────────────────────────────────────┐
│                         lnch (binary)                        │
├──────────┬───────────────────────────────────────────────────┤
│          │                                                   │
│   CLI    │              Application Core                     │
│  (clap)  │  ┌─────────────────────────────────────────────┐  │
│          │  │              App (状態管理)                   │  │
│  ┌────┐  │  │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │  │
│  │ run│──┼─▶│  │  Config  │  │ Process  │  │    TUI    │ │  │
│  └────┘  │  │  │  Loader  │  │ Manager  │  │  Renderer │ │  │
│          │  │  └────┬─────┘  └────┬─────┘  └─────┬─────┘ │  │
│          │  │       │             │               │       │  │
│          │  │       ▼             ▼               ▼       │  │
│          │  │  ┌─────────┐  ┌──────────┐  ┌───────────┐  │  │
│          │  │  │  YAML   │  │  tokio   │  │  ratatui  │  │  │
│          │  │  │  File   │  │ process  │  │ crossterm │  │  │
│          │  │  └─────────┘  └──────────┘  └───────────┘  │  │
│          │  └─────────────────────────────────────────────┘  │
└──────────┴───────────────────────────────────────────────────┘
```

## コンポーネント間の通信

コンポーネント間は **tokioチャネル (`mpsc`)** を用いた非同期メッセージパッシングで通信する。

```
┌───────────┐    AppEvent     ┌───────────┐   ProcessCmd    ┌──────────────┐
│  TUI      │ ──────────────▶ │   App     │ ─────────────▶  │   Process    │
│  (Event   │                 │  (State   │                  │   Manager    │
│   Loop)   │ ◀────────────── │  Machine) │ ◀─────────────── │              │
└───────────┘    RenderTick   └───────────┘  ProcessEvent    └──────┬───────┘
                                                                    │
                                                         ┌──────────┼──────────┐
                                                         ▼          ▼          ▼
                                                    ┌────────┐ ┌────────┐ ┌────────┐
                                                    │ Task 1 │ │ Task 2 │ │ Task N │
                                                    │Process │ │Process │ │Process │
                                                    └────────┘ └────────┘ └────────┘
```

## イベントフロー

```
1. ユーザーキー入力
   └─▶ crossterm::event → AppEvent::Key(KeyEvent)

2. App が AppEvent を受信
   └─▶ 状態遷移を判定
       ├─▶ ProcessCmd::Start(task_name)   → ProcessManager へ
       ├─▶ ProcessCmd::Stop(task_name)    → ProcessManager へ
       ├─▶ ProcessCmd::Restart(task_name) → ProcessManager へ
       ├─▶ ProcessCmd::StartAll           → ProcessManager へ
       └─▶ ProcessCmd::Shutdown           → 全プロセス停止 → アプリ終了

3. ProcessManager からの通知
   └─▶ ProcessEvent::StatusChanged { task, status }
   └─▶ ProcessEvent::LogLine { task, line }
   └─▶ ProcessEvent::ExitCode { task, code }

4. App が ProcessEvent を受信
   └─▶ TaskState を更新 → 次の描画フレームに反映
```

---

## ディレクトリ構成

```
lnch/
├── Cargo.toml
├── Cargo.lock
├── LICENSE
├── README.md
├── docs/
│   ├── README.md              # ドキュメントインデックス
│   ├── 01-overview.md
│   ├── 02-architecture.md
│   ├── 03-config.md
│   ├── 04-process-management.md
│   ├── 05-tui.md
│   ├── 06-cli-and-error-handling.md
│   ├── 07-testing.md
│   └── 08-roadmap.md
├── src/
│   ├── main.rs                # エントリーポイント
│   ├── cli.rs                 # CLI定義 (clap)
│   ├── config/
│   │   ├── mod.rs             # モジュール公開定義
│   │   ├── model.rs           # 設定データ構造体
│   │   ├── loader.rs          # YAML読み込み・ファイル探索
│   │   └── validator.rs       # 設定値バリデーション
│   ├── process/
│   │   ├── mod.rs             # モジュール公開定義
│   │   ├── manager.rs         # ProcessManager（全タスクの統括）
│   │   ├── task_runner.rs     # 個別タスクの起動・停止・IO
│   │   ├── pty.rs             # Windows ConPTY ラッパー (Windows のみ)
│   │   ├── dependency.rs      # depends_on の解決（トポロジカルソート）
│   │   └── signal.rs          # シグナルハンドリング・クリーンアップ
│   ├── tui/
│   │   ├── mod.rs             # モジュール公開定義
│   │   ├── app.rs             # アプリケーション状態管理
│   │   ├── event.rs           # イベントハンドラ（キー入力→アクション変換）
│   │   ├── ui.rs              # UI描画ロジック（レイアウト・ウィジェット配置）
│   │   └── widgets/
│   │       ├── mod.rs         # ウィジェットモジュール公開定義
│   │       ├── task_list.rs   # タスク一覧ウィジェット
│   │       ├── log_view.rs    # ログ表示ウィジェット
│   │       └── status_bar.rs  # ステータスバーウィジェット
│   └── log/
│       ├── mod.rs             # モジュール公開定義
│       └── buffer.rs          # リングバッファによるログ保持
└── tests/
    ├── config_test.rs         # 設定ファイルのパース・バリデーションテスト
    ├── dependency_test.rs     # 依存関係解決テスト
    ├── process_test.rs        # プロセス管理テスト
    └── fixtures/
        ├── valid.yaml         # 正常系テスト用設定ファイル
        ├── invalid.yaml       # 異常系テスト用設定ファイル
        └── circular_dep.yaml  # 循環依存テスト用設定ファイル
```

---

## Cargo.toml 依存関係（MVP）

```toml
[package]
name = "lnch"
version = "0.1.0"
edition = "2021"
description = "A TUI multi-process launcher for your dev environment"
license = "MIT"
repository = "https://github.com/<user>/lnch"
keywords = ["tui", "process-manager", "dev-tools", "launcher"]
categories = ["command-line-utilities", "development-tools"]

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
crossterm = "0.28"
nix = { version = "0.29", features = ["signal", "process"] }
ratatui = "0.29"
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_System_Threading", "Win32_Foundation"] }
```
