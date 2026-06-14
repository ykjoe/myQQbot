#!/usr/bin/env python3
"""
Command handler for QQ bot.

Reads JSON from stdin:  {"content": "...", "node_name": "..."}
Writes JSON to stdout: {"reply": "..."} or {"reply": null}

Idle timeout: exits automatically after 30s of inactivity.
"""

import json
import sys
import signal


# ── Idle timeout ─────────────────────────────────────────────────────

IDLE_TIMEOUT_SEC = 30

def _handle_timeout(signum, frame):
    sys.exit(0)

signal.signal(signal.SIGALRM, _handle_timeout)


# ── Helpers ──────────────────────────────────────────────────────────

def cpu():
    with open("/proc/stat") as f:
        parts = f.readline().split()
    if len(parts) < 5:
        return "N/A"
    user, nice, system, idle = int(parts[1]), int(parts[2]), int(parts[3]), int(parts[4])
    total = user + nice + system + idle
    used = user + nice + system
    pct = used * 100 // total if total > 0 else 0
    return f"{pct}%"

def mem():
    with open("/proc/meminfo") as f:
        data = f.read()
    total_kb = 0
    avail_kb = 0
    for line in data.splitlines():
        if line.startswith("MemTotal:"):
            total_kb = int(line.split()[1])
        elif line.startswith("MemAvailable:"):
            avail_kb = int(line.split()[1])
    if total_kb == 0:
        return "N/A"
    used_mb = (total_kb - avail_kb) // 1024
    total_mb = total_kb // 1024
    pct = (total_kb - avail_kb) * 100 // total_kb
    return f"{used_mb}MB / {total_mb}MB ({pct}%)"

def disk():
    total_read = 0
    total_write = 0
    with open("/proc/diskstats") as f:
        for line in f:
            parts = line.split()
            if len(parts) >= 10:
                try:
                    total_read += int(parts[3])
                    total_write += int(parts[7])
                except ValueError:
                    pass
    return f"{total_read}R / {total_write}W (IO count)"


# ── Command definitions ──────────────────────────────────────────────
# Each command: (trigger_substring, handler(node_name) -> str | None)
# The handler directly builds the reply string, calling helpers as needed.

COMMANDS = [
    ("/你好",
    lambda n: (
        f"【{n}】🤖 机器人已就绪\n\n"
        f"━━━━━━📊 服务器状态━━━━━━\n"
        f"💻 CPU  : {cpu()}\n"
        f"🧠 内存 : {mem()}\n"
        f"💾 硬盘 : {disk()}\n"
        f"━━━━━━━━━━━━━━━━━━"
    )),
]


# ── Main ─────────────────────────────────────────────────────────────

def main():
    sys.stdout.reconfigure(line_buffering=True)
    sys.stdin.reconfigure(line_buffering=True)

    signal.alarm(IDLE_TIMEOUT_SEC)

    for line in sys.stdin:
        signal.alarm(0)
        line = line.strip()
        if not line:
            signal.alarm(IDLE_TIMEOUT_SEC)
            continue

        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            print(json.dumps({"reply": None}), flush=True)
            signal.alarm(IDLE_TIMEOUT_SEC)
            continue

        content = req.get("content", "")
        node_name = req.get("node_name", "")

        reply = None
        for trigger, handler in COMMANDS:
            if trigger in content:
                reply = handler(node_name)
                break

        print(json.dumps({"reply": reply}), flush=True)
        signal.alarm(IDLE_TIMEOUT_SEC)


if __name__ == "__main__":
    main()
