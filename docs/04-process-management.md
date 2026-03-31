# プロセス管理設計

## 1. タスクのライフサイクル

```
          start()             プロセス起動完了
Stopped ──────────▶ Starting ─────────────────▶ Running
   ▲                                              │
   │                                              │ プロセス終了 (exit_code != 0)
   │        stop()                                ▼
   ├◀───── Stopping ◀─────────────────────────  Failed
   │          ▲                                    │
   │          │              stop()                │
   │          └────────────────────────────────────┘
   │
   │        プロセスが正常終了 (exit_code == 0)
   └◀───────────────────────────────────────── Running
```

---

## 2. ProcessManager

**責務**: 全タスクのライフサイクル管理を統括

```rust
pub struct ProcessManager {
    tasks: HashMap<String, TaskRunner>,
    dependency_graph: DependencyGraph,
    cmd_rx: mpsc::Receiver<ProcessCommand>,
    event_tx: mpsc::Sender<ProcessEvent>,
}

impl ProcessManager {
    pub async fn new(
        config: &LnchConfig,
        cmd_rx: mpsc::Receiver<ProcessCommand>,
        event_tx: mpsc::Sender<ProcessEvent>,
    ) -> Self { /* ... */ }

    /// メインループ: コマンドを受信して処理
    pub async fn run(&mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                ProcessCommand::Start(name) => self.start_task(&name).await,
                ProcessCommand::Stop(name) => self.stop_task(&name).await,
                ProcessCommand::Restart(name) => self.restart_task(&name).await,
                ProcessCommand::StartAll => self.start_all().await,
                ProcessCommand::StopAll => self.stop_all().await,
                ProcessCommand::Shutdown => {
                    self.stop_all().await;
                    break;
                }
            }
        }
    }

    /// depends_on を考慮した起動順序で全タスクを開始
    /// 各グループ起動後、ready_check（またはスマートデフォルト）で
    /// 準備完了を待ってから次のグループを起動する
    async fn start_all(&mut self) {
        let groups = self.dependency_graph.topological_sort();
        for (idx, group) in groups.iter().enumerate() {
            for name in group {
                self.start_task(name).await;
            }
            // 最終グループ以外は準備完了を待つ
            if idx < groups.len() - 1 {
                self.wait_for_group_ready(group).await;
            }
        }
    }

    /// グループ内全タスクの準備完了を並行で待機
    async fn wait_for_group_ready(&self, group: &[String]) {
        // 各タスクの wait_ready() を並行実行
        // タイムアウト時は警告ログを出して続行
    }
}
```

---

## 3. TaskRunner — 個別タスク実行

**責務**: 1つのタスクの起動・停止・ログ収集

```rust
pub struct TaskRunner {
    config: TaskConfig,
    child: Option<tokio::process::Child>,
    status: TaskStatus,
    event_tx: mpsc::Sender<ProcessEvent>,
}

impl TaskRunner {
    /// タスクを起動
    pub async fn start(&mut self) -> anyhow::Result<()> {
        // 1. ステータスを Starting に変更
        self.set_status(TaskStatus::Starting).await;

        // 2. Commandを構築
        let mut cmd = self.build_command();

        // 3. プロセスグループを新規作成（Unix: setsid相当）
        self.configure_process_group(&mut cmd);

        // 4. stdout/stderr をパイプで取得
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // 5. 子プロセスを起動
        let mut child = cmd.spawn()?;

        // 6. stdout/stderr の非同期読み取りタスクを生成
        self.spawn_log_reader(child.stdout.take().unwrap(), false);
        self.spawn_log_reader(child.stderr.take().unwrap(), true);

        // 7. プロセス終了監視タスクを生成
        self.spawn_exit_monitor(child);

        // 8. ステータスを Running に変更
        self.set_status(TaskStatus::Running).await;

        Ok(())
    }

    /// タスクを停止（グレースフルシャットダウン）
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(ref child) = self.child {
            self.set_status(TaskStatus::Stopping).await;

            // 1. SIGTERM をプロセスグループに送信
            self.send_signal_to_group(Signal::SIGTERM)?;

            // 2. 最大5秒間、終了を待機
            match tokio::time::timeout(
                Duration::from_secs(5),
                self.wait_for_exit()
            ).await {
                Ok(_) => { /* 正常終了 */ }
                Err(_) => {
                    // 3. タイムアウト: SIGKILL を送信
                    self.send_signal_to_group(Signal::SIGKILL)?;
                }
            }

            self.set_status(TaskStatus::Stopped).await;
        }
        Ok(())
    }

    /// OS に応じた Command を構築
    fn build_command(&self) -> tokio::process::Command {
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg(flag).arg(&self.config.command);

        if let Some(ref dir) = self.config.working_dir {
            cmd.current_dir(dir);
        }

        if let Some(ref env_vars) = self.config.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        cmd
    }
}
```

---

## 4. 依存関係解決 (`depends_on`)

**責務**: `depends_on` のトポロジカルソートと循環検出

```rust
pub struct DependencyGraph {
    /// タスク名 → 依存先タスク名のリスト
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn from_config(config: &LnchConfig) -> anyhow::Result<Self> { /* ... */ }

    /// Kahn's algorithm によるトポロジカルソート
    /// 戻り値: Vec<Vec<String>> — 同じ深さ（並列起動可能）のグループのリスト
    pub fn topological_sort(&self) -> Vec<Vec<String>> { /* ... */ }

    /// 循環依存の検出（DFS）
    pub fn detect_cycle(&self) -> Option<Vec<String>> { /* ... */ }
}
```

**起動順序の例:**

```yaml
tasks:
  - name: database
  - name: backend
    depends_on: [database]
  - name: frontend
    depends_on: [backend]
  - name: worker
    depends_on: [database, backend]
```

```
解決結果:
  Group 0: [database]         ← 最初に起動
  Group 1: [backend]          ← database 起動後
  Group 2: [frontend, worker] ← backend 起動後（並列起動可能）
```

### depends_on の起動制御フロー

```
1. 設定読み込み時に DependencyGraph を構築
2. 循環依存が検出された場合はエラーで起動中止
3. StartAll 時:
   a. トポロジカルソートで起動順序を決定（グループ単位）
   b. 依存のないタスク群（グループ0）を並列起動
   c. ready_check に基づいて準備完了を待機:
      - ready_check 指定あり: tcp / http / log_line / exit チェック
      - ready_check 未指定（スマートデフォルト）:
        * プロセスが2秒以内に exit 0 → 一発タスク完了で ready
        * プロセスが2秒以上生存 → 常駐サービス、grace period 経過で ready
        * プロセスが2秒以内に異常終了 → failed（警告して続行）
      - タイムアウト時: 警告ログを出して次グループへ続行
   d. 次の依存レベルのタスク群を並列起動
   e. 繰り返し（最終グループは待機不要）
4. 個別 Start 時:
   a. 依存先が全て Running であることを確認
   b. 未起動の依存先がある場合、先に起動する
```

---

## 5. ConPTY によるプロセス起動 (Windows)

### 背景

`Stdio::piped()` で子プロセスの stdout/stderr をキャプチャすると、ハンドルは**匿名パイプ**になる。
Windows で子プロセスが `multiprocessing.spawn`（Python）などを使い `bInheritHandles=FALSE` で孫プロセスを生成した場合、パイプハンドルは孫プロセスに継承されず `OSError: [Errno 22] Invalid argument` が発生する。

一方、**コンソールハンドル（疑似ハンドル）** は同じコンソールに接続された全プロセスで自動的に有効であり、明示的なハンドル継承を必要としない。

### ConPTY の仕組み

Windows 10 version 1809 以降で利用可能な ConPTY（Pseudo Console）API を使い、子プロセスに仮想コンソールを提供する。

```
lnch (TUI)
 └─ [ConPTY (仮想コンソール)]
      ├─ 出力パイプ → lnch が読み取って TUI に表示
      ├─ 入力パイプ → Ctrl+C 送信用に保持
      └─ コンソール側 → 子プロセスが接続
           └─ uvicorn メインプロセス (stdout/stderr = コンソールハンドル)
                └─ ワーカープロセス (コンソールハンドルは継承不要で有効) ✓
```

### 実装

- `src/process/pty.rs` — `PtyProcess` が ConPTY のライフサイクルを管理
- `TaskRunner::start()` は Windows でまず ConPTY モード（`start_with_pty()`）を試行し、失敗時は従来の pipe モード（`start_with_pipes()`）にフォールバック
- ConPTY の出力パイプは `tokio::task::spawn_blocking` 内でブロッキング読み取りを行う
- 出力に含まれる ANSI エスケープシーケンスは `strip-ansi-escapes` クレートで除去

### 制約

- **stdout/stderr の区別不可**: ConPTY は単一のストリームに統合するため、全出力が stdout として扱われる
- **Windows 10 1809 以降が必要**: ConPTY 非対応環境では自動的に pipe モードにフォールバック

---

## 6. プロセスグループ管理

ゾンビプロセスの発生を防ぐため、各タスクのプロセスは**新しいプロセスグループ**で起動する。

### Unix (macOS / Linux)

```rust
use std::os::unix::process::CommandExt;

fn configure_process_group(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid().map_err(|e| std::io::Error::new(
                std::io::ErrorKind::Other, e
            ))?;
            Ok(())
        });
    }
}

fn kill_process_group(pid: u32) {
    nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM
    ).ok();
}
```

### Windows

**pipe モード**（フォールバック時）:

```rust
use windows_sys::Win32::System::Threading::*;

fn configure_process_group(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
}
```

**ConPTY モード**（デフォルト）:

ConPTY モードでは仮想コンソールがプロセスグループの役割を果たすため、`CREATE_NEW_PROCESS_GROUP` は不要。`ClosePseudoConsole` により ConHost がプロセスツリーのクリーンアップを行う。

---

## 7. グレースフルシャットダウン

### タスク停止フロー

```
1. シグナル送信:
   - Unix: SIGTERM をプロセスグループに送信
   - Windows (ConPTY モード): Ctrl+C (\x03) を ConPTY 入力パイプに書き込み
   - Windows (pipe モード): CTRL_BREAK_EVENT をプロセスグループに送信
2. 最大 5 秒間、プロセスの終了を待機
3. タイムアウトした場合:
   a. SIGKILL（Unix）/ taskkill /F /T（Windows）を送信
   b. 1 秒間待機
   c. プロセスの終了を確認
4. ステータスを Stopped に更新
```

### アプリケーション終了フロー

```
Ctrl+C / SIGTERM 受信
  │
  ├─▶ shutdown フラグを立てる
  │
  ├─▶ 全子プロセスに SIGTERM 送信（プロセスグループ単位）
  │
  ├─▶ 5秒間待機
  │     ├─ 全プロセス終了 → 正常終了
  │     └─ タイムアウト → SIGKILL 送信
  │
  ├─▶ ターミナルの復元（raw mode 解除、alternate screen 終了）
  │
  └─▶ exit(0)
```

---

## 8. シグナルハンドリング

**責務**: アプリケーション終了時の安全なクリーンアップ

```rust
/// Ctrl+C / SIGTERM のハンドラをセットアップ
pub async fn setup_signal_handler(shutdown_tx: mpsc::Sender<()>) {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();
        let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt()).unwrap();
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        }
        let _ = shutdown_tx.send(()).await;
    }

    #[cfg(windows)]
    {
        tokio::signal::ctrl_c().await.unwrap();
        let _ = shutdown_tx.send(()).await;
    }
}
```
