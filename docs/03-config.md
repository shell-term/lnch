# 設定ファイル・データモデル設計

## 1. 設定ファイルスキーマ (`lnch.yaml`)

```yaml
# プロジェクト名（TUIのタイトルバーに表示）
name: my-project  # Required: string

# タスク定義（1つ以上必須）
tasks:
  - name: frontend              # Required: string（一意）
    command: npm run dev         # Required: string（シェル経由で実行）
    working_dir: ./frontend     # Optional: string（lnch.yaml からの相対パス or 絶対パス）
    env:                        # Optional: map<string, string>
      NEXT_PUBLIC_API_URL: http://localhost:8080
      NODE_ENV: development
    color: green                # Optional: enum (後述)
    depends_on:                 # Optional: list<string>（タスク名の参照）
      - database
    ready_check:               # Optional: object（依存関係の準備完了チェック）
      tcp: { port: 5432 }     # tcp / http / log_line / exit のいずれか1つ
      timeout: 30              # Optional: タイムアウト秒数（デフォルト: 30）
      interval: 500            # Optional: ポーリング間隔ミリ秒（デフォルト: 500）
```

## 2. フィールド詳細

| フィールド | 型 | 必須 | デフォルト | 説明 |
|-----------|-----|------|-----------|------|
| `name` (root) | `string` | ✅ | — | プロジェクト名。TUIタイトルバーに表示 |
| `tasks` | `list<Task>` | ✅ | — | タスク定義のリスト。1つ以上必須 |
| `tasks[].name` | `string` | ✅ | — | タスク名。タスクリスト内で一意であること |
| `tasks[].command` | `string` | ✅ | — | 実行コマンド。OS標準シェル経由で実行 |
| `tasks[].working_dir` | `string` | ❌ | `lnch.yaml` のあるディレクトリ | 作業ディレクトリ |
| `tasks[].env` | `map<string, string>` | ❌ | `{}` | 追加の環境変数（親プロセスの環境変数を継承した上で上書き） |
| `tasks[].color` | `string` | ❌ | 自動割当 | タスクのテーマカラー |
| `tasks[].depends_on` | `list<string>` | ❌ | `[]` | 先に起動すべきタスク名のリスト |
| `tasks[].ready_check` | `ReadyCheckConfig` | ❌ | スマートデフォルト | 依存関係の準備完了チェック設定 |

## 3. カラー定義

`color` フィールドに指定可能な値:

| 値 | ANSI Color |
|----|-----------|
| `red` | Red |
| `green` | Green |
| `yellow` | Yellow |
| `blue` | Blue |
| `magenta` | Magenta |
| `cyan` | Cyan |
| `white` | White |

未指定時は、タスクの定義順に以下の順序で自動割当:
`green` → `blue` → `yellow` → `magenta` → `cyan` → `red` → `white` → (繰り返し)

## 4. 設定ファイル探索アルゴリズム

`lnch` コマンドが引数なしで実行された場合、以下の順序で `lnch.yaml` を探索する:

```
1. $CWD/lnch.yaml を確認
2. 見つからない場合、親ディレクトリへ移動
3. ファイルシステムのルートに到達するまで繰り返す
4. 見つからなければエラー:
   "lnch.yaml not found. Run 'lnch init' to create one,
    or specify a file with 'lnch --config <path>'."
```

**探索の打ち切り条件:**
- ファイルシステムルート (`/` or `C:\`) に到達
- 最大探索深度: 10階層（無限ループ防止）

## 5. コマンド実行方式

`command` フィールドの値はシェル経由で実行する:

| OS | 実行方式 |
|----|---------|
| macOS / Linux | `sh -c "<command>"` |
| Windows | `cmd /C "<command>"` |

シェル経由にすることで、パイプ (`|`)、リダイレクト (`>`)、環境変数展開 (`$VAR`) 等のシェル構文をそのまま利用可能にする。

---

## 6. 設定モデル（デシリアライズ用）

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LnchConfig {
    pub name: String,
    pub tasks: Vec<TaskConfig>,
}

#[derive(Debug, Deserialize)]
pub struct TaskConfig {
    pub name: String,
    pub command: String,
    pub working_dir: Option<PathBuf>,
    pub env: Option<HashMap<String, String>>,
    pub color: Option<String>,
    pub depends_on: Option<Vec<String>>,
    pub ready_check: Option<ReadyCheckConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ReadyCheckConfig {
    pub tcp: Option<TcpCheck>,
    pub http: Option<HttpCheck>,
    pub log_line: Option<LogLineCheck>,
    pub exit: Option<ExitCheck>,
    pub timeout: Option<u64>,    // 秒（デフォルト: 30）
    pub interval: Option<u64>,   // ミリ秒（デフォルト: 500）
}

#[derive(Debug, Deserialize)]
pub struct TcpCheck { pub port: u16 }

#[derive(Debug, Deserialize)]
pub struct HttpCheck { pub url: String, pub status: Option<u16> }

#[derive(Debug, Deserialize)]
pub struct LogLineCheck { pub pattern: String }

#[derive(Debug, Deserialize)]
pub struct ExitCheck {}
```

## 7. ランタイム状態モデル

```rust
use tokio::sync::mpsc;

/// タスクの実行状態
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed { exit_code: Option<i32> },
}

/// タスクのランタイム状態
pub struct TaskState {
    pub config: TaskConfig,
    pub status: TaskStatus,
    pub pid: Option<u32>,
    pub log_buffer: LogBuffer,
}

/// アプリケーション全体の状態
pub struct AppState {
    pub project_name: String,
    pub tasks: Vec<TaskState>,
    pub selected_index: usize,
    pub log_scroll_offset: usize,
    pub should_quit: bool,
}
```

## 8. メッセージ型

```rust
/// App → ProcessManager へのコマンド
pub enum ProcessCommand {
    Start(String),           // タスク名
    Stop(String),            // タスク名
    Restart(String),         // タスク名
    StartAll,
    StopAll,
    Shutdown,
}

/// ProcessManager → App への通知
pub enum ProcessEvent {
    StatusChanged {
        task_name: String,
        status: TaskStatus,
    },
    LogLine {
        task_name: String,
        line: String,
        is_stderr: bool,
    },
    ProcessExited {
        task_name: String,
        exit_code: Option<i32>,
    },
}

/// TUI イベント
pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Tick,
    Process(ProcessEvent),
}
```

## 9. ログバッファ

```rust
/// 固定サイズのリングバッファ
pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    capacity: usize,
}

pub struct LogLine {
    pub content: String,
    pub is_stderr: bool,
    pub timestamp: Instant,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self { /* ... */ }
    pub fn push(&mut self, line: LogLine) { /* ... */ }
    pub fn lines(&self) -> &VecDeque<LogLine> { /* ... */ }
    pub fn len(&self) -> usize { /* ... */ }
    pub fn clear(&mut self) { /* ... */ }
}
```

**バッファサイズ**: タスクあたりデフォルト `10,000` 行。設定ファイルで変更可能にはしない（MVP）。

---

## 10. モジュール設計

### `config/loader.rs` — 設定ファイル読み込み

**責務**: 設定ファイルの探索・読み込み・パース

```rust
/// カレントディレクトリから上位に向かって lnch.yaml を探索
pub fn find_config() -> anyhow::Result<PathBuf> {
    let mut current = std::env::current_dir()?;
    let max_depth = 10;

    for _ in 0..max_depth {
        let candidate = current.join("lnch.yaml");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    anyhow::bail!(
        "lnch.yaml not found.\n\
         Run 'lnch init' to create one, or specify a file with 'lnch --config <path>'."
    )
}

/// YAML ファイルを読み込んで LnchConfig にデシリアライズ
pub fn load_config(path: &Path) -> anyhow::Result<LnchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: LnchConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}
```

### `config/validator.rs` — バリデーション

**責務**: 設定値の整合性チェック

| # | チェック内容 | エラーメッセージ |
|---|-------------|----------------|
| 1 | `tasks` が空でないこと | `"No tasks defined in config"` |
| 2 | タスク名が一意であること | `"Duplicate task name: '{name}'"` |
| 3 | `depends_on` の参照先が存在すること | `"Task '{name}' depends on unknown task '{dep}'"` |
| 4 | `depends_on` に循環がないこと | `"Circular dependency detected: {cycle}"` |
| 5 | `color` が有効な値であること | `"Invalid color '{color}' for task '{name}'"` |
| 6 | `working_dir` が存在するディレクトリであること | `"Working directory does not exist: '{dir}'"` |
| 7 | `ready_check` のチェック種類が1つだけ指定されていること | `"ready_check must specify exactly one of: tcp, http, log_line, exit"` |
| 8 | `ready_check.http.url` が空でないこと | `"ready_check http url must not be empty"` |
| 9 | `ready_check.log_line.pattern` が空でないこと | `"ready_check log_line pattern must not be empty"` |
