"""Robsidian のプラグインの例。

標準入力から JSON のリクエストを1つ読み、標準出力に JSON のレスポンスを1つ書く。
形は docs/ROADMAP.md の「フェーズE」を参照。
"""

import csv
import datetime
import io
import json
import re
import sys


def respond(markdown=None, actions=None):
    out = {}
    if markdown is not None:
        out["markdown"] = markdown
    if actions:
        out["actions"] = actions
    json.dump(out, sys.stdout, ensure_ascii=False)


def command(req):
    note = req.get("note") or {}
    if req["id"] == "upper":
        text = note.get("selection") or ""
        if not text:
            return respond(actions=[{"type": "notice", "text": "文字を選んでから実行してください"}])
        return respond(actions=[{"type": "replace_selection", "text": text.upper()}])
    if req["id"] == "stamp":
        today = datetime.date.today().isoformat()
        return respond(actions=[{"type": "insert", "text": today}])


def codeblock(req):
    # CSV を Markdown の表にする（1行目を見出しにする）
    rows = [r for r in csv.reader(io.StringIO(req.get("code") or "")) if r]
    if not rows:
        return respond(markdown="*（空の表）*")
    width = max(len(r) for r in rows)
    rows = [r + [""] * (width - len(r)) for r in rows]
    esc = lambda s: s.replace("|", "\\|")
    lines = ["| " + " | ".join(map(esc, rows[0])) + " |", "|" + " --- |" * width]
    lines += ["| " + " | ".join(map(esc, r)) + " |" for r in rows[1:]]
    respond(markdown="\n".join(lines) + "\n")


def panel(req):
    text = (req.get("note") or {}).get("text") or ""
    chars = len(re.sub(r"\s", "", text))
    headings = len(re.findall(r"^#{1,6} ", text, re.M))
    links = len(re.findall(r"\[\[[^\]]+\]\]", text))
    tasks = len(re.findall(r"^\s*[-*+] \[ \]", text, re.M))
    respond(
        markdown=(
            f"- 文字数: **{chars}**\n"
            f"- 見出し: {headings}\n"
            f"- リンク: {links}\n"
            f"- 未完了のタスク: {tasks}\n"
        )
    )


def event(req):
    # 保存したとき: frontmatter があれば updated を今日にする（変わらなければ何もしない）
    note = req.get("note") or {}
    text = note.get("text") or ""
    if req["id"] != "save" or not text.startswith("---\n"):
        return respond()
    end = text.find("\n---", 4)
    if end < 0:
        return respond()
    head, body = text[4:end], text[end:]
    today = datetime.date.today().isoformat()
    if re.search(r"^updated:", head, re.M):
        new_head = re.sub(r"^updated:.*$", f"updated: {today}", head, flags=re.M)
    else:
        new_head = head + f"\nupdated: {today}"
    if new_head == head:
        return respond()
    respond(actions=[{"type": "replace_note", "text": "---\n" + new_head + body}])


def main():
    req = json.load(sys.stdin)
    {"command": command, "codeblock": codeblock, "panel": panel, "event": event}[req["kind"]](req)


main()
