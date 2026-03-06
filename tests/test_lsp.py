import json
import sys
import subprocess
import os

def send_message(proc, msg_dict):
    body = json.dumps(msg_dict).encode('utf-8')
    header = f"Content-Length: {len(body)}\r\n\r\n".encode('utf-8')
    proc.stdin.write(header + body)
    proc.stdin.flush()

def read_message(proc):
    # Read headers
    content_length = 0
    while True:
        line = proc.stdout.readline().decode('utf-8')
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.startswith("Content-Length: "):
            content_length = int(line[16:])
            
    if content_length > 0:
        body = proc.stdout.read(content_length).decode('utf-8')
        return json.loads(body)
    return None

def main():
    kivm_exe = os.path.join(os.path.dirname(__file__), "..", "target", "debug", "kivm.exe")
    print(f"Starting {kivm_exe} lsp")
    
    proc = subprocess.Popen([kivm_exe, "lsp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    
    # 1. Initialize
    send_message(proc, {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    
    resp = read_message(proc)
    print("Initialize Response:", resp)
    
    # 2. didChange with broken code
    send_message(proc, {
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.kix"},
            "contentChanges": [{"text": "let x = \n// missing value"}]
        }
    })
    
    resp = read_message(proc)
    print("Diagnostics Response:", resp)
    
    # 3. Shutdown
    send_message(proc, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": {}
    })
    
    resp = read_message(proc)
    print("Shutdown Response:", resp)
    
    # 4. Exit
    send_message(proc, {
        "jsonrpc": "2.0",
        "method": "exit",
        "params": {}
    })
    
    # wait for exit
    proc.wait()
    print("Exited with:", proc.returncode)
    
    # Print what was on stderr
    print("Stderr:\n", proc.stderr.read().decode('utf-8'))

if __name__ == "__main__":
    main()
