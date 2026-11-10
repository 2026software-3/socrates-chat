# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scikit-learn", "scipy", "httpx"]
# ///
"""第二輪：不預設分群答案。embedding 總結後分群並由 LLM 命名＋描述，對比 LLM 直接分組；以盲測評審計分。
用法：uv run run2.py <apikeylist> <emb|llm|judge|report>"""
import json
import sys
import time
from pathlib import Path

import numpy as np
from sklearn.cluster import AgglomerativeClustering, KMeans
from sklearn.metrics import silhouette_score

_stage = sys.argv[2]
_keyfile = sys.argv[1]
sys.argv = ["run.py", _keyfile, "x"]
import run  # noqa: E402
from data2 import TOPICS  # noqa: E402

HERE = Path(__file__).parent
MODELS = ["gpt-6-luna", "gpt-5.6-luna"]  # 正式設定：主模型被限流就換備援
_dead = set()


class Limited(Exception):
    pass


def post_once(path, body):
    key = run.KEYS[0]
    for attempt in range(3):
        r = run.client.post(run.BASE + path, json=body, headers={"Authorization": f"Bearer {key}"})
        if r.status_code == 200:
            return r.json()
        if r.status_code == 429:
            if "per day" in r.text or "RPD" in r.text:
                raise Limited(body.get("model"))
            time.sleep(8 * (attempt + 1))
            continue
        if r.status_code >= 500:
            time.sleep(3)
            continue
        raise RuntimeError(f"HTTP {r.status_code}: {r.text[:300].replace(key, '<key>')}")
    raise Limited(body.get("model"))


run.post = post_once


def chat(system, user, tag):
    """先讀任一模型的快取；沒有才呼叫，主模型每日額度用完就換備援。"""
    bodies = [
        {"model": m, "reasoning_effort": "medium",
         "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]}
        for m in MODELS
    ]
    last = None
    for offline in (True, False):
        for body in bodies:
            if not offline and body["model"] in _dead:
                continue
            try:
                if offline:
                    saved, run.post = run.post, lambda p, b: (_ for _ in ()).throw(Limited("offline"))
                    try:
                        out = run.cached(tag, body, "/chat/completions")
                    finally:
                        run.post = saved
                else:
                    out = run.cached(tag, body, "/chat/completions")
            except Limited as e:
                last = e
                if not offline:
                    _dead.add(body["model"])
                continue
            u = out["usage"]
            return out["choices"][0]["message"]["content"] or "", {
                "model": body["model"], "in": u["prompt_tokens"], "out": u["completion_tokens"],
                "secs": round(out["_secs"], 1),
            }
    raise Limited(f"all models limited ({last})")


def summary_text(it):
    return f"立場：{it[0]}\n理由：{it[1]}"


def vectors(name, model="text-embedding-3-large"):
    t = TOPICS[name]
    X, toks = run.embed([summary_text(it) for it in t["items"]], model)
    Xc = X - X.mean(axis=0)
    return Xc / np.linalg.norm(Xc, axis=1, keepdims=True), toks


def auto_cluster(X, algo):
    """不指定組數：在 k=2..12 中取輪廓係數最高者（不需要任何標註）。"""
    best = None
    for k in range(2, 13):
        if algo == "kmeans":
            lab = KMeans(n_clusters=k, n_init=20, random_state=0).fit_predict(X)
        else:
            lab = AgglomerativeClustering(n_clusters=k, metric="cosine", linkage="average").fit_predict(X)
        s = silhouette_score(X, lab, metric="cosine")
        if best is None or s > best[0]:
            best = (s, k, lab)
    return best


def groups_of(lab):
    out = {}
    for i, l in enumerate(lab):
        out.setdefault(int(l), []).append(i)
    return sorted(out.values(), key=lambda g: (-len(g), g))


def stage_emb():
    res = {}
    for name, t in TOPICS.items():
        X, toks = vectors(name)
        res[name] = {"tokens": toks}
        for algo in ["kmeans", "agglo"]:
            s, k, lab = auto_cluster(X, algo)
            res[name][algo] = {"sil": round(float(s), 3), "k": k, "groups": groups_of(lab)}
            print(f"\n== {name} {algo}: k={k} silhouette={s:.3f} tokens={toks}")
            for g in groups_of(lab):
                print("  ---")
                for i in g:
                    print(f"   {i + 1:2d}. {t['items'][i][0]} {t['items'][i][1]}")
    (HERE / "r2_emb.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))


# ---- LLM ----
def regroup_summary_prompt(topic, items):
    lst = "\n".join(f"{i + 1}. 立場：{it[0]} 理由：{it[1]}" for i, it in enumerate(items))
    return (
        f"你在整理學生對討論題目「{topic}」的論點，只使用繁體中文。以下是每位學生的總結（立場與核心理由，已編號）。\n"
        "請把論點相同或相近的學生歸成一群，每群取一個簡短的名稱（12 字內，描述論點本身），並用一兩句話描述這群人共同的立場與理由；\n"
        "不限制群組數量與分法，每個編號只能屬於一個群組。\n"
        f"總結：\n{lst}\n"
        '只輸出 JSON：{"groups":[{"name":"群組名稱","description":"描述","members":[1,2]}]}'
    )


def naming_prompt(topic, items, groups):
    blocks = []
    for gi, g in enumerate(groups):
        blocks.append(f"群組 {gi + 1}：\n" + "\n".join(f"- 立場：{items[i][0]} 理由：{items[i][1]}" for i in g))
    return (
        f"你在整理學生對討論題目「{topic}」的論點，只使用繁體中文。以下學生總結已經分好群。\n"
        "請替每一群取一個簡短的立場名稱（12 字內，描述論點本身），並用一兩句話描述這群人共同的立場與理由。\n"
        + "\n".join(blocks)
        + '\n只輸出 JSON：{"groups":[{"id":1,"name":"立場名稱","description":"描述"}]}'
    )


def parse_groups(raw, n):
    j = run.extract_json(raw) or {}
    seen, out = set(), []
    for g in j.get("groups", []):
        ms = [m - 1 for m in g.get("members", []) if isinstance(m, int) and 1 <= m <= n and m - 1 not in seen]
        seen.update(ms)
        if ms:
            out.append({"name": g.get("name", ""), "description": g.get("description", ""), "members": ms})
    for i in range(n):
        if i not in seen:
            out.append({"name": "（未分組）", "description": "", "members": [i]})
    return out


def stage_llm():
    emb = json.loads((HERE / "r2_emb.json").read_text())
    res = json.loads((HERE / "r2_llm.json").read_text()) if (HERE / "r2_llm.json").exists() else {}
    for name, t in TOPICS.items():
        items, topic, n = t["items"], t["topic"], len(t["items"])
        r = res.setdefault(name, {})
        try:
            if "emb" not in r:  # embedding 分群 → LLM 命名＋描述
                groups = emb[name][PRIMARY]["groups"]
                raw, u = chat(naming_prompt(topic, items, groups), "請命名並描述。", "r2-naming")
                j = run.extract_json(raw) or {}
                meta = {g.get("id"): g for g in j.get("groups", [])}
                r["emb"] = {"usage": u, "groups": [
                    {"name": meta.get(i + 1, {}).get("name", ""), "description": meta.get(i + 1, {}).get("description", ""), "members": g}
                    for i, g in enumerate(groups)]}
            if "llm_summary" not in r:  # LLM 直接看總結分組
                raw, u = chat(regroup_summary_prompt(topic, items), "請分組。", "r2-regroup-summary")
                r["llm_summary"] = {"usage": u, "groups": parse_groups(raw, n)}
            if "llm_claim" not in r:  # 現行做法：正式 prompt，只看主張短句
                raw, u = chat(run.regroup_prompt(topic, [it[2] for it in items]), "請分組。", "r2-regroup-claim")
                r["llm_claim"] = {"usage": u, "groups": parse_groups(raw, n)}
        except Limited as e:
            print("LIMITED:", e, "at", name, flush=True)
            break
        finally:
            (HERE / "r2_llm.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
        print("done", name, {k: v["usage"] for k, v in r.items()}, flush=True)


PRIMARY = "kmeans"


def judge_prompt(topic, items, groups):
    blocks = []
    for gi, g in enumerate(groups):
        blocks.append(f"群組 {gi + 1}：\n" + "\n".join(f"  [{i + 1}] 立場：{items[i][0]} 理由：{items[i][1]}" for i in g["members"]))
    return (
        f"你是嚴格的評審，只使用繁體中文。以下是學生對「{topic}」的總結，已被某個方法分成若干群。\n"
        "評分標準只有一個：同一群的學生是否持有相同或相近的論點（立場與理由都要考慮）。不限制群組數量與分法。\n"
        "請逐群檢查：\n"
        "1. misfits：這一群裡論點明顯與該群多數人不同、不該放在一起的學生編號（方括號內的數字）。"
        "如果一群裡混了兩種以上不同的論點，以人數較多的為主，其餘都列為 misfits；人數相同時任選一邊為主。單獨一人的群沒有 misfits。\n"
        "2. merge：論點實質相同、應該合併的群組配對（群組編號）。只有幾乎是同一個論點才算，僅立場方向相同但理由不同不算。\n"
        + "\n".join(blocks)
        + '\n只輸出 JSON：{"groups":[{"id":1,"misfits":[3]}],"merge":[[1,2]]}'
    )


def stage_judge():
    llm = json.loads((HERE / "r2_llm.json").read_text())
    res = json.loads((HERE / "r2_judge.json").read_text()) if (HERE / "r2_judge.json").exists() else {}
    for name, t in TOPICS.items():
        for method, g in llm.get(name, {}).items():
            key = f"{name}/{method}"
            if key in res:
                continue
            try:
                raw, u = chat(judge_prompt(t["topic"], t["items"], g["groups"]), "請評審。", "r2-judge")
            except Limited as e:
                print("LIMITED:", e, "at", key, flush=True)
                (HERE / "r2_judge.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
                return
            j = run.extract_json(raw) or {}
            misfits = sorted({m for gg in j.get("groups", []) for m in gg.get("misfits", [])})
            res[key] = {"usage": u, "misfits": misfits, "merge": j.get("merge", [])}
            (HERE / "r2_judge.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
            print("judged", key, res[key], flush=True)


def stage_report():
    llm = json.loads((HERE / "r2_llm.json").read_text())
    judge = json.loads((HERE / "r2_judge.json").read_text()) if (HERE / "r2_judge.json").exists() else {}
    for name, t in TOPICS.items():
        for method, g in llm.get(name, {}).items():
            jd = judge.get(f"{name}/{method}")
            print(f"\n==== {name} / {method}  groups={len(g['groups'])} usage={g['usage']} judge={jd and {k: jd[k] for k in ('misfits', 'merge')}}")
            for gi, gg in enumerate(g["groups"]):
                print(f" [{gi + 1}] {gg['name']} ({len(gg['members'])}) — {gg['description']}")
                print("      " + " ".join(str(i + 1) for i in gg["members"]))


if __name__ == "__main__":
    {"emb": stage_emb, "llm": stage_llm, "judge": stage_judge, "report": stage_report}[_stage]()
