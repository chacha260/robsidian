"""常駐するプラグインの例: 今週のタスクのまとめと、このノートへのつながり。

- コマンド「今週のタスクをまとめる」: vault/search で今週が期日のタスクを集め、
  「今週のタスク.md」を vault/write で書いて開く
- パネル「つながり」: vault/backlinks でこのノートへのリンクを数える。
  起動したままなので、呼ばれた回数（状態）も覚えている
"""

import robsidian

calls = 0


@robsidian.handler
def handle(req):
    global calls
    calls += 1
    if req["kind"] == "command" and req["id"] == "weekly":
        found = robsidian.call("vault/search", query="task:todo due:week sort:due", limit=100)
        tasks = found.get("tasks", [])
        lines = ["# 今週のタスク", ""]
        for t in tasks:
            name = t["path"].removesuffix(".md")
            lines.append(f"- [ ] {t['text']} 📅 {t['due']}（[[{name}]]）")
        if not tasks:
            lines.append("今週が期日のタスクはありません。")
        robsidian.call("vault/write", path="今週のタスク.md", text="\n".join(lines) + "\n")
        return {
            "actions": [
                {"type": "notice", "text": f"{len(tasks)}件のタスクをまとめました"},
                {"type": "open", "path": "今週のタスク"},
            ]
        }
    if req["kind"] == "panel":
        note = (req.get("note") or {}).get("path")
        if not note:
            return {"markdown": "*ノートを開いてください*"}
        links = robsidian.call("vault/backlinks", path=note)
        sources = sorted({b["source"] for b in links})
        body = "\n".join(f"- [[{s.removesuffix('.md')}]]" for s in sources) or "*このノートへのリンクはありません*"
        return {"markdown": f"**{len(sources)}件のノートから**\n\n{body}\n\n*起動してから{calls}回目の依頼*"}
    return {}


robsidian.run()
