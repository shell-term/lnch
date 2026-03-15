import time
import random
from celery import Celery

app = Celery(
    "tasks",
    broker="redis://localhost:6379/0",
    backend="redis://localhost:6379/0",
)


@app.task
def process_event(event_name: str):
    duration = random.uniform(0.5, 2.0)
    time.sleep(duration)
    return {"event": event_name, "processed": True}


@app.task
def generate_report(report_name: str):
    duration = random.uniform(1.0, 3.0)
    time.sleep(duration)
    return {"report": report_name, "rows": random.randint(100, 10000)}
