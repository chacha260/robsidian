"""常駐するプラグイン（mode = "server"）を Python で書くための小さな助け。

使い方:

    import robsidian

    @robsidian.handler
    def handle(req):              # Robsidian からの依頼（形は段階1と同じ）
        text = robsidian.call("vault/read", path="a.md")["text"]
        robsidian.notify("ui/notice", text="読みました")
        return {"markdown": "..."}

    robsidian.run()

やりとりは行区切りの JSON-RPC 2.0（標準入出力）。print は使わないこと（出力が壊れる）。
デバッグの出力は sys.stderr へ。
"""

import itertools
import json
import sys

_handler = None
_ids = itertools.count(1)


def handler(fn):
    """依頼を受ける関数を登録する（デコレータ）"""
    global _handler
    _handler = fn
    return fn


def _send(msg):
    msg["jsonrpc"] = "2.0"
    sys.stdout.write(json.dumps(msg, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _recv():
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    return json.loads(line)


def call(method, **params):
    """Robsidian に問い合わせて、返事を待つ（vault/read・vault/list・vault/search・vault/backlinks・vault/write）"""
    my_id = f"q{next(_ids)}"
    _send({"id": my_id, "method": method, "params": params})
    while True:
        msg = _recv()
        if msg.get("id") == my_id and "method" not in msg:
            if "error" in msg:
                raise RuntimeError(msg["error"].get("message", "失敗しました"))
            return msg.get("result")
        # 返事を待つあいだに別の依頼は来ない（Robsidian は1つずつ待たない作りでも、ここでは順に処理する）
        _dispatch(msg)


def notify(method, **params):
    """Robsidian に知らせる（ui/notice・panel/refresh）。返事は無い"""
    _send({"method": method, "params": params})


def _dispatch(msg):
    method = msg.get("method")
    if method == "plugin/request":
        try:
            result = _handler(msg["params"]) or {}
            _send({"id": msg["id"], "result": result})
        except Exception as e:  # プラグインの誤りは Robsidian に返す（プロセスは止めない）
            _send({"id": msg["id"], "error": {"code": -32000, "message": str(e)}})
    elif method == "initialize":
        _send({"id": msg["id"], "result": {}})
    elif method == "shutdown":
        sys.exit(0)


def run():
    """依頼を待ち続ける"""
    while True:
        _dispatch(_recv())
