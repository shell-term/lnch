import time
import sys

print("Worker starting...", flush=True)
counter = 0

try:
    while True:
        counter += 1
        ts = time.strftime("%H:%M:%S")
        print(f"[{ts}] Job #{counter} processed", flush=True)
        time.sleep(3)
except KeyboardInterrupt:
    print("Worker shutting down.", flush=True)
