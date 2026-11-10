# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scikit-learn", "scipy", "httpx"]
# ///
"""第四輪：完整總結 embedding 下，階層式分群與 k-means 的比對。
替階層式分群的各組補上 AI 命名與描述，並補齊兩者「只看立場」的評審。用法：uv run run4.py <apikeylist>"""
import json
import sys
from pathlib import Path

keyfile = sys.argv[1]
sys.argv = ["run2.py", keyfile, "none"]
import run2  # noqa: E402,F401
from run2 import Limited, TOPICS, chat, naming_prompt, run  # noqa: E402

HERE = Path(__file__).parent
llm = json.loads((HERE / "r2_llm.json").read_text())
r3 = json.loads((HERE / "r3.json").read_text())


def judge_prompt(topic, items, groups):  # 與 run3.py 的評審 prompt 相同
    blocks = [
        f"群組 {gi + 1}：\n" + "\n".join(f"  [{i + 1}] 立場：{items[i][0]} 理由：{items[i][1]}" for i in g["members"])
        for gi, g in enumerate(groups)
    ]
    return (
        f"你是嚴格的評審，只使用繁體中文。以下是學生對「{topic}」的總結，已被某個方法分成若干群。\n"
        "評分標準只有一個：同一群的學生是否持有相同或相近的立場。只看立場（最終主張什麼），不看理由；立場相同但理由不同，應視為同一群。不限制群組數量。\n"
        "請檢查：\n"
        "1. misfits：立場與該群多數人明顯不同、不該放在一起的學生編號（方括號內的數字）。一群裡混了兩種以上立場時，以人數較多的為主，其餘列為 misfits；人數相同時任選一邊為主。單獨一人的群沒有 misfits。\n"
        "2. merge：立場實質相同、應該合併的群組配對（群組編號）。只因理由不同而被拆開的群要列出。\n"
        + "\n".join(blocks)
        + '\n只輸出 JSON：{"groups":[{"id":1,"misfits":[3]}],"merge":[[1,2]]}'
    )


def stance_stats(groups, merge):
    n = len(groups)
    p = list(range(n))

    def f(x):
        while p[x] != x:
            x = p[x]
        return x

    for a, b in merge:
        if 1 <= a <= n and 1 <= b <= n:
            p[f(a - 1)] = f(b - 1)
    comps = {}
    for i in range(n):
        comps.setdefault(f(i), []).append(i)
    sep = sum(
        sum(len(groups[i]["members"]) for i in c) - max(len(groups[i]["members"]) for i in c)
        for c in comps.values()
    )
    return len(comps), sep


def save():
    (HERE / "r2_llm.json").write_text(json.dumps(llm, ensure_ascii=False, indent=1))
    (HERE / "r3.json").write_text(json.dumps(r3, ensure_ascii=False, indent=1))


try:
    for name, t in TOPICS.items():  # 階層式分群：命名與描述（prompt 與 k-means 那組相同）
        g = llm[name]["emb_agglo"]
        if not g.get("usage"):
            raw, u = chat(
                naming_prompt(t["topic"], t["items"], [x["members"] for x in g["groups"]]),
                "請命名並描述。",
                "r2-naming",
            )
            meta = {x.get("id"): x for x in (run.extract_json(raw) or {}).get("groups", [])}
            for i, x in enumerate(g["groups"]):
                x["name"] = meta.get(i + 1, {}).get("name", "")
                x["description"] = meta.get(i + 1, {}).get("description", "")
            g["usage"] = u
            save()
            print("named", name, u, flush=True)
        r3[name]["emb_agglo"] = {**r3[name].get("emb_agglo", {}), "groups": g["groups"], "usage": g["usage"]}
        save()
    for key in ["emb_agglo", "emb_summary"]:  # 只看立場的評審
        for name, t in TOPICS.items():
            r = r3[name][key]
            if "judge" in r:
                continue
            raw, u = chat(judge_prompt(t["topic"], t["items"], r["groups"]), "請評審。", "r3-judge")
            j = run.extract_json(raw) or {}
            merge = j.get("merge", [])
            stances, sep = stance_stats(r["groups"], merge)
            r["judge"] = {
                "misfits": sorted({m for g in j.get("groups", []) for m in g.get("misfits", [])}),
                "merge": merge,
                "usage": u,
                "stances": stances,
                "separated": sep,
            }
            save()
            print("judged", name, key, len(r["groups"]), r["judge"]["misfits"], merge, u["model"], flush=True)
except Limited as e:
    save()
    print("LIMITED:", e, flush=True)
