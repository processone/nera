"""Small stdio LSP client: initialize, open an Erlang file, request its outline."""

import json
from pathlib import Path
import subprocess
import sys
import threading


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: lsp_probe.py /path/to/nera /path/to/module.erl")
    binary = Path(sys.argv[1]).resolve()
    source = Path(sys.argv[2]).resolve()
    with subprocess.Popen(
        [str(binary), "lsp", "--transport", "stdio"],
        cwd=source.parent,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as server:
        # Bound the whole session so a broken server cannot leave this probe hanging.
        deadline = threading.Timer(30, server.kill)
        deadline.start()

        def send(method, params=None, request_id=None):
            message = {"jsonrpc": "2.0", "method": method}
            if params is not None:
                message["params"] = params
            if request_id is not None:
                message["id"] = request_id
            payload = json.dumps(message).encode("utf-8")
            server.stdin.write(
                f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii") + payload
            )
            server.stdin.flush()

        def receive_until(predicate):
            while True:
                headers = {}
                while True:
                    line = server.stdout.readline()
                    if not line:
                        raise RuntimeError("LSP closed its output or session timed out")
                    if line == b"\r\n":
                        break
                    key, value = line.decode("ascii").split(":", 1)
                    headers[key.lower()] = value.strip()
                message = json.loads(server.stdout.read(int(headers["content-length"])))
                if predicate(message):
                    if "error" in message:
                        raise RuntimeError(message["error"])
                    return message

        try:
            send("initialize", {
                "processId": None,
                "rootUri": source.parent.as_uri(),
                "capabilities": {},
            }, 1)
            capabilities = receive_until(lambda m: m.get("id") == 1)["result"]["capabilities"]
            assert capabilities["documentSymbolProvider"]
            print("LSP initialized: document symbols supported")
            send("initialized", {})
            send("textDocument/didOpen", {"textDocument": {
                "uri": source.as_uri(), "languageId": "erlang", "version": 1,
                "text": source.read_text(encoding="utf-8"),
            }})
            receive_until(lambda m: m.get("method") == "textDocument/publishDiagnostics"
                          and m["params"]["uri"] == source.as_uri())
            send("textDocument/documentSymbol", {"textDocument": {"uri": source.as_uri()}}, 2)
            symbols = receive_until(lambda m: m.get("id") == 2)["result"]
            names = sorted(symbol["name"] for symbol in symbols)
            assert "start/0" in names, names
            print("Document symbols: " + ", ".join(names))
            send("shutdown", request_id=3)
            receive_until(lambda m: m.get("id") == 3)
            send("exit")
            server.stdin.close()
            assert server.wait(timeout=5) == 0
            print("LSP shutdown: ok")
        finally:
            deadline.cancel()
            if server.poll() is None:
                server.kill()


if __name__ == "__main__":
    main()
