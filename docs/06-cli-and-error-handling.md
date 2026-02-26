# CLI・エラーハンドリング設計

## 1. エントリーポイント（`main.rs`）

**責務**: CLI引数の解析 → 設定ファイルの読み込み → アプリケーション起動

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. CLI引数を解析
    let cli = Cli::parse();

    // 2. 設定ファイルを読み込み
    let config_path = cli.config.unwrap_or_else(|| find_config().unwrap());
    let config = load_config(&config_path)?;
    validate_config(&config)?;

    // 3. TUIアプリケーションを起動
    let app = App::new(config);
    app.run().await?;

    Ok(())
}
```

---

## 2. CLI 定義（`cli.rs`）

**責務**: clapによるコマンドライン引数・サブコマンドの定義

```rust
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lnch")]
#[command(about = "A TUI multi-process launcher for your dev environment")]
#[command(version)]
pub struct Cli {
    /// 設定ファイルのパスを明示的に指定
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}
```

### コマンド体系（MVP）

```
lnch                           # TUIを起動（lnch.yaml を自動検出）
lnch --config <path>           # 指定した設定ファイルで TUI を起動
lnch --version                 # バージョン表示
lnch --help                    # ヘルプ表示
```

### 終了コード

| コード | 意味 |
|--------|------|
| `0` | 正常終了 |
| `1` | 設定ファイルが見つからない / パースエラー |
| `2` | バリデーションエラー（循環依存等） |
| `130` | ユーザーによる中断 (Ctrl+C) |

---

## 3. エラー分類

| カテゴリ | 例 | 処理方針 |
|---------|-----|---------|
| **起動時エラー** | 設定ファイルが見つからない、YAML構文エラー、バリデーションエラー | エラーメッセージを表示して終了 |
| **ランタイムエラー** | タスクの起動失敗（コマンドが見つからない等） | タスクを `Failed` 状態にし、TUIにエラー表示。他のタスクは継続 |
| **シグナルエラー** | プロセスグループへのシグナル送信失敗 | ログに警告を出力し、フォールバック処理 |
| **TUIエラー** | ターミナルの初期化失敗 | エラーメッセージを表示して終了 |

---

## 4. エラー型定義

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LnchError {
    #[error("Config file not found")]
    ConfigNotFound,

    #[error("Failed to parse config: {0}")]
    ConfigParse(#[from] serde_yaml::Error),

    #[error("Config validation error: {0}")]
    ConfigValidation(String),

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Failed to start task '{task}': {source}")]
    TaskStart {
        task: String,
        source: std::io::Error,
    },

    #[error("Terminal initialization failed: {0}")]
    TerminalInit(#[from] std::io::Error),
}
```

---

## 5. タスク起動失敗時の挙動

```
1. TaskRunner::start() が Err を返す
2. ProcessManager がタスクステータスを Failed に変更
3. ProcessEvent::StatusChanged を App に送信
4. TUI のタスクリストで Failed アイコン（✕）と赤色で表示
5. ログビューにエラーメッセージを表示
6. 他のタスクは影響を受けず継続動作
7. depends_on でこのタスクに依存しているタスクは起動しない
```
