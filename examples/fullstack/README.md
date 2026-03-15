# fullstack example

A demo that launches FastAPI + Celery worker + React (Vite) together with lnch.

## Startup order

```
redis  ──┬──▶ backend ──▶ frontend
         └──▶ worker
```

## Prerequisites

- [lnch](https://github.com/shell-term/lnch) installed
- WSL (Ubuntu) recommended
- Python 3.11+
- [uv](https://docs.astral.sh/uv/getting-started/installation/)
- Node.js 18+

## Setup

### 1. Install Redis

```bash
sudo apt update && sudo apt install -y redis-server
```

### 2. Install Python dependencies

```bash
cd backend
uv sync
cd ..
```

### 3. Install frontend dependencies

```bash
cd frontend
npm install
cd ..
```

## Run

```bash
lnch
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Select next task |
| `k` / `↑` | Select previous task |
| `s` | Start / stop selected task |
| `r` | Restart selected task |
| `q` | Quit |
