#!/usr/bin/env python3
"""Create multiple TCP connections then hold them open for N seconds."""
import socket, time, threading, sys

PORT = 19999
N = int(sys.argv[1]) if len(sys.argv) > 1 else 5
HOLD = int(sys.argv[2]) if len(sys.argv) > 2 else 6

conns = []

def server():
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('127.0.0.1', PORT))
    s.listen(20)
    while True:
        c, _ = s.accept()
        conns.append(c)

t = threading.Thread(target=server, daemon=True)
t.start()
time.sleep(0.2)

clients = []
for i in range(N):
    c = socket.socket()
    c.connect(('127.0.0.1', PORT))
    clients.append(c)

print(f"[test_connections] {N} connections established on 127.0.0.1:{PORT}", flush=True)
time.sleep(HOLD)
print("[test_connections] done", flush=True)
