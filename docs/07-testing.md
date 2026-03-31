# テスト戦略

## 1. テストレベル

| レベル | 対象 | 手法 |
|--------|------|------|
| **ユニットテスト** | 設定パーサ、バリデーション、依存関係解決、ログバッファ | `#[cfg(test)]` モジュール |
| **統合テスト** | プロセス起動・停止、シグナルハンドリング | `tests/` ディレクトリ、実際のプロセス起動 |
| **手動テスト** | TUI表示、キーバインド、エンドツーエンド | 開発者がターミナルで実行 |

---

## 2. ユニットテスト詳細

### config テスト

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_minimal_config() { /* name + 1 task のみ */ }

    #[test]
    fn test_parse_full_config() { /* 全フィールド指定 */ }

    #[test]
    fn test_parse_invalid_yaml() { /* 不正なYAML構文 */ }

    #[test]
    fn test_validate_duplicate_task_names() { /* 重複タスク名 */ }

    #[test]
    fn test_validate_unknown_dependency() { /* 存在しないタスクへのdepends_on */ }

    #[test]
    fn test_validate_circular_dependency() { /* A→B→C→A の循環 */ }

    #[test]
    fn test_validate_self_dependency() { /* A→A の自己参照 */ }
}
```

### dependency テスト

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_topological_sort_no_deps() { /* 依存なし: 全タスクが同一グループ */ }

    #[test]
    fn test_topological_sort_linear() { /* A→B→C: 3グループ */ }

    #[test]
    fn test_topological_sort_diamond() { /* A→B,C / B,C→D: 3グループ */ }

    #[test]
    fn test_detect_cycle() { /* 循環検出 */ }
}
```

### log buffer テスト

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_push_within_capacity() { /* 容量内の追加 */ }

    #[test]
    fn test_push_overflow_drops_oldest() { /* 容量超過時に古い行が破棄 */ }

    #[test]
    fn test_clear() { /* バッファクリア */ }
}
```

---

## 3. 統合テスト

```rust
// tests/process_test.rs

#[tokio::test]
async fn test_start_and_stop_simple_process() {
    // "sleep 60" を起動 → 停止 → プロセスが存在しないことを確認
}

#[tokio::test]
async fn test_process_group_kill() {
    // シェルスクリプトが子プロセスを生成 → 親を kill
    // → 子プロセスも終了していることを確認
}

#[tokio::test]
async fn test_log_capture() {
    // "echo hello" を起動 → ログバッファに "hello" が含まれることを確認
}

#[tokio::test]
async fn test_depends_on_start_order() {
    // A depends_on B → B が先に起動することを確認
}
```

### ready_check テスト

```rust
// src/process/ready.rs #[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_wait_smart_default_long_running() { /* 常駐→grace period後ready */ }

    #[tokio::test]
    async fn test_wait_smart_default_exit_success() { /* 一発タスク正常終了→ready */ }

    #[tokio::test]
    async fn test_wait_smart_default_exit_failure() { /* 一発タスク異常終了→failed */ }

    #[tokio::test]
    async fn test_wait_exit_success() { /* exit check正常終了 */ }

    #[tokio::test]
    async fn test_wait_exit_timeout() { /* exitタイムアウト */ }

    #[tokio::test]
    async fn test_wait_tcp_success() { /* TCPポートlisten成功 */ }

    #[tokio::test]
    async fn test_wait_tcp_timeout() { /* TCPタイムアウト */ }

    #[tokio::test]
    async fn test_wait_log_line_already_matched() { /* パターン即時マッチ */ }

    #[tokio::test]
    async fn test_wait_log_line_match_during_wait() { /* 待機中にパターンマッチ */ }

    #[tokio::test]
    async fn test_wait_log_line_timeout() { /* ログパターンタイムアウト */ }
}
```

```rust
// tests/ready_check_test.rs（統合テスト）

#[tokio::test]
async fn test_smart_default_oneshot_dependency() { /* 一発タスク依存→exit後に次グループ起動 */ }

#[tokio::test]
async fn test_smart_default_long_running_dependency() { /* 常駐タスク依存→grace period後に次グループ */ }

#[tokio::test]
async fn test_exit_ready_check() { /* 明示的exit check経由で起動順序保証 */ }

#[tokio::test]
async fn test_log_line_ready_check() { /* ログパターンマッチで次グループ起動 */ }

#[tokio::test]
async fn test_tcp_ready_check() { /* TCP listenで次グループ起動 */ }

#[tokio::test]
async fn test_ready_check_timeout_continues() { /* タイムアウト後も続行 */ }

#[tokio::test]
async fn test_no_deps_all_start_immediately() { /* 依存なし→全タスク同時起動 */ }

#[tokio::test]
async fn test_multi_level_dependency_chain() { /* a→b→c 3段チェーン */ }
```

### update checker テスト

```rust
// src/update/checker.rs #[cfg(test)]
mod tests {
    #[test]
    fn test_is_newer_patch() { /* 0.1.7 < 0.1.8 */ }

    #[test]
    fn test_is_newer_minor() { /* 0.1.7 < 0.2.0 */ }

    #[test]
    fn test_is_newer_major() { /* 0.1.7 < 1.0.0 */ }

    #[test]
    fn test_not_newer_same() { /* 同一バージョン → false */ }

    #[test]
    fn test_not_newer_older() { /* 古いバージョン → false */ }

    #[test]
    fn test_install_command_contains_version() { /* コマンドにバージョン番号含む */ }

    #[test]
    fn test_parse_github_response() { /* GitHub API JSON → tag_name パース */ }
}
```

---

## 4. テストフィクスチャ

```
tests/fixtures/
├── valid.yaml           # 正常系テスト用設定ファイル
├── invalid.yaml         # 異常系テスト用設定ファイル（不正なYAML構文）
└── circular_dep.yaml    # 循環依存テスト用設定ファイル
```

### `valid.yaml`

```yaml
name: test-project
tasks:
  - name: task-a
    command: echo "hello from A"
    color: green
  - name: task-b
    command: echo "hello from B"
    depends_on: [task-a]
    color: blue
```

### `circular_dep.yaml`

```yaml
name: circular-test
tasks:
  - name: a
    command: echo a
    depends_on: [c]
  - name: b
    command: echo b
    depends_on: [a]
  - name: c
    command: echo c
    depends_on: [b]
```
