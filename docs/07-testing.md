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
