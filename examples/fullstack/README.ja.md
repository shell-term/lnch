# fullstack サンプル

FastAPI + Celery worker + React (Vite) を lnch で一括起動するデモです。

## 起動の流れ

```
redis  ──┬──▶ backend ──▶ frontend
         └──▶ worker
```

## 前提条件

- [lnch](https://github.com/shell-term/lnch) がインストール済み
- WSL (Ubuntu) 推奨
- Python 3.11+
- [uv](https://docs.astral.sh/uv/getting-started/installation/)
- Node.js 18+

## セットアップ

### 1. Redis をインストール

```bash
sudo apt update && sudo apt install -y redis-server
```

### 2. Python 依存パッケージをインストール

```bash
cd backend
uv sync
cd ..
```

### 3. フロントエンドの依存パッケージをインストール

```bash
cd frontend
npm install
cd ..
```

## 起動

```bash
lnch
```

## キーバインド

| キー | 操作 |
|------|------|
| `j` / `↓` | 次のタスクを選択 |
| `k` / `↑` | 前のタスクを選択 |
| `s` | 選択タスクの起動 / 停止 |
| `r` | 選択タスクの再起動 |
| `q` | 終了 |
