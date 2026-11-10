# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scikit-learn", "scipy", "httpx"]
# ///
"""第三輪：只按立場分組。用法：uv run run3.py <apikeylist>"""
import json, sys
from pathlib import Path
import numpy as np
keyfile = sys.argv[1]
sys.argv = ["run2.py", keyfile, "none"]
import run2
from run2 import run, TOPICS, chat, Limited, auto_cluster, groups_of
HERE = Path(__file__).parent
OUT = HERE / "r3.json"
res = json.loads(OUT.read_text()) if OUT.exists() else {}
llm = json.loads((HERE / "r2_llm.json").read_text())

def cluster(name, field):
    t = TOPICS[name]
    X, toks = run.embed([f"題目：{t['topic']}\n立場：{it[field]}" for it in t["items"]], "text-embedding-3-large")
    X = X - X.mean(axis=0); X = X / np.linalg.norm(X, axis=1, keepdims=True)
    s, k, lab = auto_cluster(X, "kmeans")
    return groups_of(lab), toks

def naming_prompt(topic, items, groups):
    blocks = [f"群組 {gi + 1}：\n" + "\n".join(f"- 立場：{items[i][0]} 理由：{items[i][1]}" for i in g) for gi, g in enumerate(groups)]
    return (f"你在整理學生對討論題目「{topic}」的立場，只使用繁體中文。以下學生總結已經依立場分好群。\n"
            "請替每一群取一個簡短的立場名稱（12 字內，描述立場本身），並用一兩句話描述這群人的共同立場，以及他們提出的主要理由。\n"
            + "\n".join(blocks) + '\n只輸出 JSON：{"groups":[{"id":1,"name":"立場名稱","description":"描述"}]}')

def judge_prompt(topic, items, groups):
    blocks = [f"群組 {gi + 1}：\n" + "\n".join(f"  [{i + 1}] 立場：{items[i][0]} 理由：{items[i][1]}" for i in g["members"]) for gi, g in enumerate(groups)]
    return (f"你是嚴格的評審，只使用繁體中文。以下是學生對「{topic}」的總結，已被某個方法分成若干群。\n"
            "評分標準只有一個：同一群的學生是否持有相同或相近的立場。只看立場（最終主張什麼），不看理由；立場相同但理由不同，應視為同一群。不限制群組數量。\n"
            "請檢查：\n"
            "1. misfits：立場與該群多數人明顯不同、不該放在一起的學生編號（方括號內的數字）。一群裡混了兩種以上立場時，以人數較多的為主，其餘列為 misfits；人數相同時任選一邊為主。單獨一人的群沒有 misfits。\n"
            "2. merge：立場實質相同、應該合併的群組配對（群組編號）。只因理由不同而被拆開的群要列出。\n"
            + "\n".join(blocks) + '\n只輸出 JSON：{"groups":[{"id":1,"misfits":[3]}],"merge":[[1,2]]}')

def save(): OUT.write_text(json.dumps(res, ensure_ascii=False, indent=1))

try:
    for name, t in TOPICS.items():
        items, topic = t["items"], t["topic"]
        r = res.setdefault(name, {})
        for key, field in [("emb_claim", 2), ("emb_stance", 0)]:
            if key not in r:
                groups, toks = cluster(name, field)
                r[key] = {"embtok": toks, "groups": [{"name": "", "description": "", "members": g} for g in groups]}
        for key, src in [("llm_claim", "llm_claim"), ("llm_summary", "llm_summary"), ("emb_summary", "emb")]:
            r.setdefault(key, {"groups": llm[name][src]["groups"], "usage": llm[name][src]["usage"]})
        save()
    order = ["emb_claim", "llm_claim", "emb_stance", "llm_summary", "emb_summary"]
    for name, t in TOPICS.items():  # 先命名
        r = res[name]["emb_claim"]
        if "usage" not in r:
            raw, u = chat(naming_prompt(t["topic"], t["items"], [g["members"] for g in r["groups"]]), "請命名並描述。", "r3-naming")
            meta = {g.get("id"): g for g in (run.extract_json(raw) or {}).get("groups", [])}
            for i, g in enumerate(r["groups"]):
                g["name"] = meta.get(i + 1, {}).get("name", ""); g["description"] = meta.get(i + 1, {}).get("description", "")
            r["usage"] = u; save(); print("named", name, u, flush=True)
    for key in order:
        for name, t in TOPICS.items():
            r = res[name][key]
            if "judge" in r: continue
            raw, u = chat(judge_prompt(t["topic"], t["items"], r["groups"]), "請評審。", "r3-judge")
            j = run.extract_json(raw) or {}
            r["judge"] = {"misfits": sorted({m for g in j.get("groups", []) for m in g.get("misfits", [])}), "merge": j.get("merge", []), "usage": u}
            save(); print("judged", name, key, len(r["groups"]), r["judge"]["misfits"], r["judge"]["merge"], u["model"], flush=True)
except Limited as e:
    save(); print("LIMITED:", e, flush=True)
