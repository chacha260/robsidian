"""プラグインの一覧（catalog.json）を作る。

    python3 make_catalog.py <公開するフォルダ> > catalog.json

<公開するフォルダ> の下の、plugin.toml があるフォルダを1つのプラグインとして並べる。
各プラグインのファイル（隠しファイルと target/・src/・Cargo.* を除く）の sha256 を計算し、
一覧からの相対 URL で書く。catalog.json を <公開するフォルダ> に置いて公開すれば、
Robsidian の設定の「プラグイン」や `rob plugin install` から入れられる。
"""

import hashlib
import json
import pathlib
import sys
import tomllib

SKIP_DIRS = {"target", "src", "__pycache__"}


def files_of(plugin_dir: pathlib.Path):
    for p in sorted(plugin_dir.rglob("*")):
        rel = p.relative_to(plugin_dir)
        if not p.is_file() or any(part.startswith(".") for part in rel.parts):
            continue
        if rel.parts[0] in SKIP_DIRS or rel.name.startswith("Cargo."):
            continue
        yield rel


def main():
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    plugins = []
    for toml_path in sorted(root.glob("*/plugin.toml")):
        d = toml_path.parent
        meta = tomllib.loads(toml_path.read_text(encoding="utf-8"))
        files = {}
        for rel in files_of(d):
            data = (d / rel).read_bytes()
            files[rel.as_posix()] = {
                "url": f"{d.name}/{rel.as_posix()}",
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        entry = {"id": d.name, "name": meta.get("name", d.name), "files": files}
        for key in ("version", "description"):
            if key in meta:
                entry[key] = meta[key]
        plugins.append(entry)
    json.dump({"plugins": plugins}, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")


main()
