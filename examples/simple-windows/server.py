import http.server
import sys
import os

port = int(os.environ.get("PORT", "18080"))
print(f"Server starting on port {port}...", flush=True)

handler = http.server.SimpleHTTPRequestHandler
server = http.server.HTTPServer(("127.0.0.1", port), handler)
print(f"Server is running on http://127.0.0.1:{port}", flush=True)

try:
    server.serve_forever()
except KeyboardInterrupt:
    print("Server shutting down.", flush=True)
