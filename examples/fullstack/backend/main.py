import asyncio
import random
from contextlib import asynccontextmanager
from fastapi import FastAPI
from tasks import process_event, generate_report

EVENTS = [
    "user.signup",
    "order.placed",
    "payment.processed",
    "user.login",
    "file.uploaded",
]


async def _enqueue_loop():
    await asyncio.sleep(3)
    while True:
        event = random.choice(EVENTS)
        process_event.delay(event)
        await asyncio.sleep(random.uniform(4, 9))


@asynccontextmanager
async def lifespan(app: FastAPI):
    task = asyncio.create_task(_enqueue_loop())
    yield
    task.cancel()
    try:
        await task
    except asyncio.CancelledError:
        pass


app = FastAPI(title="My App API", version="1.0.0", lifespan=lifespan)


@app.get("/health")
def health():
    return {"status": "healthy", "version": "1.0.0"}


@app.get("/api/users")
def list_users():
    return [{"id": i, "name": f"User {i}"} for i in range(1, 6)]


@app.post("/api/reports")
def create_report(name: str = "monthly"):
    task = generate_report.delay(name)
    return {"task_id": task.id, "status": "queued"}
