# TUI 画面設計

## 1. メインレイアウト

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
│                     │                                               │
│                     │                                               │
│                     │                                               │
├─────────────────────┴───────────────────────────────────────────────┤
│ [a] All Start  [s] Start/Stop  [r] Restart  [↑↓] Select  [/] Search  [c] Clear  [q] Quit │
└─────────────────────────────────────────────────────────────────────┘
```

## 2. レイアウト構造

```rust
fn render(frame: &mut Frame, state: &AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),      // タイトルバー
            Constraint::Min(0),         // メインコンテンツ
            Constraint::Length(1),      // ステータスバー
        ])
        .split(frame.area());

    let main_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // 左ペイン: タスクリスト
            Constraint::Percentage(75), // 右ペイン: ログビュー
        ])
        .split(root[1]);
}
```

---

## 3. ウィジェット詳細

### タスクリスト（左ペイン）

| 要素 | 表示 | 説明 |
|------|------|------|
| ステータスアイコン | `●` / `○` / `◉` / `✕` | Running / Stopped / Starting / Failed |
| タスク名 | タスクの `name` | 設定色で着色 |
| 選択状態 | `>` プレフィックス + 背景色 | 現在選択中のタスクをハイライト |

**ステータスアイコンと色の対応:**

| 状態 | アイコン | 色 |
|------|--------|-----|
| `Running` | `●` | Green |
| `Stopped` | `○` | DarkGray |
| `Starting` | `◉` | Yellow (点滅) |
| `Stopping` | `◉` | Yellow |
| `Failed` | `✕` | Red |

### ログビュー（右ペイン）

- 選択中タスクの stdout/stderr を表示
- stderr の行は赤色で表示
- 最新ログへの自動スクロール（末尾追従モード）
- `PageUp`/`PageDown` でスクロール操作（自動スクロール一時停止）
- 新しいログが到着し、ユーザーが最下部にいる場合は自動スクロール再開
- `/` キーでログ検索（後述）

### ログ検索

vim/less ライクなインクリメンタル検索機能。

- `/` キーで検索バーを開き、文字入力でリアルタイムにマッチ箇所をハイライト
- `Enter` で検索確定、最初のマッチにジャンプ
- `n` / `N` で次/前の���ッチへ移動（循環）
- `Esc` で検索キャンセルまたはハイライト解除
- **smart case**: クエリが全て小文字なら大文字小文字を区別しない。大文字を含む場合は区別する
- マッチ箇所は黄色背景でハイライト、現在フォーカス中のマッチは赤背景で表示
- 検索バーはログビューの下部ボーダーに表示され、マッチ数と現在位置（例: `[3/15]`）を表示

```
┌─ Logs: [api] ────────────────────────────────┐
│ [2026-04-06] Server started on port 8080      │
│ [2026-04-06] Connected to database            │
│ [2026-04-06] Error: connection timeout        │  ← マッチ箇所がハイライト
└─ /error                            [1/1] ────┘
```

### 設定リロード

`Shift+R` で `lnch.yaml` を再読み込みし、変更をランタイムに反映する。lnch を再起動せずにタスクの追加・削除・変更が可能。

- 変更検出はタスク名をキーに、`TaskConfig` の全フィールドを比較
- **未変更タスク**: 実行状態とログをそのまま保持
- **変更タスク**: 停止 → 新しい設定で再作成（ログはクリア）
- **追加タスク**: 停止状態で追加（手動起動が必要）
- **削除タスク**: 停止 → 除去

リロード結果はステータスバーにフィードバック表示される：

| 状態 | 表示例 | 色 | 表示時間 |
|------|--------|-----|---------|
| 成功 | `Reloaded: 1 added, 1 changed` | 緑背景 | 3秒 |
| 変更なし | `No changes` | 緑背景 | 3秒 |
| エラー | `Reload failed: invalid config` | 赤背景 | 5秒 |

### ステータスバー（最下段）

キーバインドのヘルプを常時表示。新バージョンが検出された場合は `[u] Update` が追加される:

```
[a] All Start  [s] Start/Stop  [r] Restart  [↑↓] Select  [/] Search  [c] Clear  [u] Update  [q] Quit
```

### アップデート通知

起動時にバックグラウンドで GitHub Releases API をチェックし、新バージョンがある場合にタイトルバー右側とステータスバーに通知を表示する。

- **チェックタイミング**: 起動直後 + 1時間毎の定期チェック（非同期、TUI起動をブロックしない）
- **キャッシュ**: `~/.cache/lnch/update_check.json`（1時間TTL）で API 呼び出しを最小化
- **`[u]` キー**: TUI を終了し、プラットフォームに応じたインストーラー（`shell` / `powershell`）を実行。完了後に再起動を促すメッセージを表示
- **オプトアウト**: 環境変数 `LNCH_NO_UPDATE_CHECK=1` で無効化

---

## 4. キーバインド一覧

| キー | アクション | コンテキスト |
|------|-----------|------------|
| `q` | アプリケーション終了 | 常時 |
| `Ctrl+C` | アプリケーション終了 | 常時 |
| `a` | 全タスク起動 | 常時 |
| `s` | 選択タスクの起動/停止トグル | 常時 |
| `r` | 選択タスクの再起動 | 常時 |
| `↑` / `k` | 前のタスクを選択 | 常時 |
| `↓` / `j` | 次のタスクを選択 | 常時 |
| `PageUp` | ログを上にスクロール | 常時 |
| `PageDown` | ログを下にスクロール | 常時 |
| `Home` | ログの先頭へ | 常時 |
| `End` | ログの末尾へ（自動スクロール再開） | 常時 |
| `c` | 選択タスクのログをクリア | 常時 |
| `u` | lnch をアップデート | 新バージョン検出時 |
| `/` | ログ検索を開始 | 通常モード |
| `n` | 次の検索マッチへジャンプ | 検索結果あり |
| `N` | 前の検索マッチへジャンプ | 検索結果あり |
| `Enter` | 検索確定 | 検索入力中 |
| `Esc` | 検索キャンセル / ハイライト解除 | 検索入力中 / 検索結果あり |
| `Shift+R` | 設定ファイルのリロード | 通常モード |

---

## 5. 描画更新頻度

| イベント | 更新タイミング |
|---------|--------------|
| キー入力 | 即時 |
| ログ行受信 | バッチ処理（最大 60fps） |
| タスク状態変化 | 即時 |
| 定期更新（Tick） | 200ms 間隔 |

---

## 6. イベントループ（`tui/app.rs`）

**責務**: イベントループの統括、状態遷移の管理

```rust
pub struct App {
    state: AppState,
    process_cmd_tx: mpsc::Sender<ProcessCommand>,
    event_rx: mpsc::Receiver<AppEvent>,
}

impl App {
    pub async fn run(mut self) -> anyhow::Result<()> {
        // 1. ターミナルを初期化（raw mode, alternate screen）
        let mut terminal = setup_terminal()?;

        // 2. ProcessManager を別タスクで起動
        tokio::spawn(async move { process_manager.run().await });

        // 3. キー入力イベントの読み取りタスクを起動
        tokio::spawn(async move { event_reader.run().await });

        // 4. メインイベントループ
        loop {
            // 4a. 画面を描画
            terminal.draw(|frame| ui::render(frame, &self.state))?;

            // 4b. イベントを待機
            match self.event_rx.recv().await {
                Some(AppEvent::Key(key)) => self.handle_key(key).await,
                Some(AppEvent::Tick) => { /* 定期描画更新（200ms間隔） */ }
                Some(AppEvent::Process(event)) => self.handle_process_event(event),
                None => break,
            }

            if self.state.should_quit {
                self.process_cmd_tx.send(ProcessCommand::Shutdown).await?;
                break;
            }
        }

        // 5. ターミナルを復元
        restore_terminal()?;
        Ok(())
    }
}
```

---

## 7. イベントハンドラ（`tui/event.rs`）

**責務**: キーイベントをアプリケーションアクションに変換

```rust
impl App {
    pub async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            // タスク選択
            KeyCode::Up | KeyCode::Char('k') => self.select_previous_task(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next_task(),

            // タスク制御
            KeyCode::Char('a') => {
                self.process_cmd_tx.send(ProcessCommand::StartAll).await.ok();
            }
            KeyCode::Char('s') => {
                let name = self.selected_task_name();
                let cmd = if self.is_task_running(&name) {
                    ProcessCommand::Stop(name)
                } else {
                    ProcessCommand::Start(name)
                };
                self.process_cmd_tx.send(cmd).await.ok();
            }
            KeyCode::Char('r') => {
                let name = self.selected_task_name();
                self.process_cmd_tx.send(ProcessCommand::Restart(name)).await.ok();
            }

            // ログスクロール
            KeyCode::PageUp => self.scroll_log_up(),
            KeyCode::PageDown => self.scroll_log_down(),

            // 終了
            KeyCode::Char('q') => self.state.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.should_quit = true;
            }

            _ => {}
        }
    }
}
```

---

## 8. ログバッファ（`log/buffer.rs`）

**責務**: タスクごとのログを固定サイズのリングバッファで保持

古い行は自動的に破棄され、メモリ使用量を一定に保つ。

```rust
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

**バッファサイズ**: タスクあたりデフォルト `10,000` 行。
