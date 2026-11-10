# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scikit-learn", "scipy", "httpx"]
# ///
"""主張分組：LLM（正式 prompt）對比 embedding＋分群。用法：uv run run.py <apikeylist> <stage>"""
import hashlib
import json
import random
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import httpx
import numpy as np
from scipy.optimize import linear_sum_assignment
from sklearn.cluster import HDBSCAN, AgglomerativeClustering, KMeans
from sklearn.metrics import adjusted_rand_score, silhouette_score
from sklearn.metrics.cluster import contingency_matrix

from data import DATASETS

HERE = Path(__file__).parent
CACHE = HERE / "cache"
CACHE.mkdir(exist_ok=True)
BASE = "https://api.openai.com/v1"
CHAT_MODEL = "gpt-6-luna"
EFFORT = "medium"
KEYS = [
    l.strip()
    for l in Path(sys.argv[1]).read_text().splitlines()
    if l.strip() and not l.startswith("#")
]
_key_i = 0
_lock = threading.Lock()
client = httpx.Client(timeout=180)


def post(path, body):
    global _key_i
    for attempt in range(8):
        with _lock:
            key = KEYS[_key_i % len(KEYS)]
        r = client.post(BASE + path, json=body, headers={"Authorization": f"Bearer {key}"})
        if r.status_code == 200:
            return r.json()
        if r.status_code in (429, 500, 502, 503):
            with _lock:
                _key_i += 1
            time.sleep(2 + attempt * 2)
            continue
        raise RuntimeError(f"{path} HTTP {r.status_code}: {r.text[:300]}")
    raise RuntimeError(f"{path} gave up after retries")


def cached(tag, body, path):
    h = hashlib.sha256(json.dumps([tag, path, body], ensure_ascii=False, sort_keys=True).encode()).hexdigest()[:24]
    f = CACHE / f"{h}.json"
    if f.exists():
        return json.loads(f.read_text())
    t = time.time()
    out = post(path, body)
    out["_secs"] = time.time() - t
    f.write_text(json.dumps(out, ensure_ascii=False))
    return out


def chat(system, user, tag):
    body = {
        "model": CHAT_MODEL,
        "reasoning_effort": EFFORT,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    }
    out = cached(tag, body, "/chat/completions")
    u = out["usage"]
    return out["choices"][0]["message"]["content"] or "", {
        "in": u["prompt_tokens"],
        "out": u["completion_tokens"],
        "reasoning": (u.get("completion_tokens_details") or {}).get("reasoning_tokens", 0),
        "calls": 1,
        "secs": out["_secs"],
    }


def embed(texts, model):
    out = cached("emb", {"model": model, "input": texts}, "/embeddings")
    v = np.array([d["embedding"] for d in sorted(out["data"], key=lambda d: d["index"])])
    return v / np.linalg.norm(v, axis=1, keepdims=True), out["usage"]["prompt_tokens"]


# ---- 正式環境的 prompt（照抄 backend/src/ai.rs 的 zh-TW 版本）----
def assign_prompt(topic, existing):
    lst = "\n".join(f"- {n}" for n in existing) if existing else "（目前還沒有任何群組）"
    return (
        f"你在整理學生對討論題目「{topic}」的主張，只使用繁體中文。使用者訊息是一位學生最終主張的一句短句。\n"
        "請判斷它屬於下列哪個既有群組；意思相同或相近就優先歸入既有群組，名稱要原樣照抄。\n"
        "只有真的不屬於任何既有群組時，才開新群，並取一個簡短的名稱（12 字內，描述主張本身）。\n"
        f"既有群組：\n{lst}\n"
        '只輸出 JSON：{"group":"群組名稱"}'
    )


def regroup_prompt(topic, claims):
    lst = "\n".join(f"{i + 1}. {c}" for i, c in enumerate(claims))
    return (
        f"你在整理學生對討論題目「{topic}」的主張，只使用繁體中文。以下是每位學生最終主張的一句短句（已編號）。\n"
        "請把意思相同或相近的歸成一群，每群取一個簡短的名稱（12 字內，描述主張本身）；\n"
        "群組數量要少而有意義，每個編號只能屬於一個群組。\n"
        f"主張：\n{lst}\n"
        '只輸出 JSON：{"groups":[{"name":"群組名稱","members":[1,2]}]}'
    )


def extract_json(raw):
    try:
        return json.loads(raw[raw.index("{") : raw.rindex("}") + 1])
    except Exception:
        return None


def add(a, b):
    return {k: a.get(k, 0) + b.get(k, 0) for k in set(a) | set(b)}


def llm_regroup(topic, claims, run):
    raw, usage = chat(regroup_prompt(topic, claims), "請分組。", f"regroup-{run}")
    j = extract_json(raw)
    labels = [None] * len(claims)
    names = {}
    if j:
        for g in j.get("groups", []):
            for m in g.get("members", []):
                if isinstance(m, int) and 1 <= m <= len(claims) and labels[m - 1] is None:
                    labels[m - 1] = g["name"]
                    names[g["name"]] = names.get(g["name"], 0) + 1
    failed = sum(l is None for l in labels)
    labels = [l if l is not None else f"__u{i}" for i, l in enumerate(labels)]
    return labels, usage, names, failed


def llm_incremental(topic, claims, order_tag):
    existing, labels, usage, failed = [], [], {}, 0
    for i, c in enumerate(claims):
        raw, u = chat(assign_prompt(topic, existing), c, f"assign-{order_tag}")
        usage = add(usage, u)
        j = extract_json(raw)
        name = (j or {}).get("group", "").strip() if isinstance(j, dict) else ""
        if not name:
            failed += 1
            labels.append(f"__u{i}")
            continue
        hit = next((e for e in existing if e.strip().lower() == name.lower()), None)
        if hit is None:
            existing.append(name)
            hit = name
        labels.append(hit)
    return labels, usage, existing, failed


# ---- 分群 ----
def agglo(X, thr):
    return AgglomerativeClustering(
        n_clusters=None, distance_threshold=thr, metric="cosine", linkage="average"
    ).fit_predict(X)


def kmeans_sil(X):
    best = None
    for k in range(2, min(10, len(X) - 1) + 1):
        lab = KMeans(n_clusters=k, n_init=10, random_state=0).fit_predict(X)
        s = silhouette_score(X, lab, metric="cosine")
        if best is None or s > best[0]:
            best = (s, lab)
    return best[1]


def kmeans_k(X, k):
    return KMeans(n_clusters=k, n_init=10, random_state=0).fit_predict(X)


def hdb(X):
    lab = HDBSCAN(min_cluster_size=3, metric="cosine").fit_predict(X)
    nxt = lab.max() + 1
    out = []
    for l in lab:  # 雜訊點視為各自一群（等同「未分組」）
        if l == -1:
            out.append(nxt)
            nxt += 1
        else:
            out.append(l)
    return np.array(out)


def online(X, thr):
    """逐筆歸群：與最近的群中心相似度不足 thr 就開新群（模擬新總結進來的路徑）。"""
    sums, labels = [], []
    for x in X:
        if sums:
            cents = np.array([s / np.linalg.norm(s) for s in sums])
            sims = cents @ x
            j = int(sims.argmax())
            if sims[j] >= thr:
                sums[j] = sums[j] + x
                labels.append(j)
                continue
        sums.append(x.copy())
        labels.append(len(sums) - 1)
    return np.array(labels)


# ---- 指標 ----
def matched_acc(truth, pred):
    """匈牙利演算法一對一配對後的正確率：每個真實群只能對到一個預測群。"""
    m = contingency_matrix(truth, pred)
    r, c = linear_sum_assignment(-m)
    return m[r, c].sum() / m.sum()


def score(truth, pred):
    return {
        "ari": adjusted_rand_score(truth, pred),
        "acc": matched_acc(truth, pred),
        "k": len(set(pred)),
    }


def load(name):
    d = DATASETS[name]
    items = [(c, g) for g, cs in d["groups"].items() for c in cs]
    random.Random(7).shuffle(items)
    return d["topic"], [c for c, _ in items], [g for _, g in items]


def stage_llm():
    res = {}

    def one(name):
        topic, claims, truth = load(name)
        out = {"regroup": [], "incremental": []}
        for run in range(3):
            labels, usage, names, failed = llm_regroup(topic, claims, run)
            out["regroup"].append({**score(truth, labels), "usage": usage, "names": names, "failed": failed, "labels": labels})
        for order in range(2):
            idx = list(range(len(claims)))
            if order:
                random.Random(99).shuffle(idx)
            labels_o, usage, names, failed = llm_incremental(topic, [claims[i] for i in idx], f"o{order}")
            labels = [None] * len(claims)
            for pos, i in enumerate(idx):
                labels[i] = labels_o[pos]
            out["incremental"].append({**score(truth, labels), "usage": usage, "names": names, "failed": failed, "labels": labels})
        print("done", name, flush=True)
        return name, out

    with ThreadPoolExecutor(4) as ex:
        for name, out in ex.map(one, DATASETS):
            res[name] = out
    (HERE / "llm_results.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))


def stage_emb():
    res = {}
    thrs = [round(x, 3) for x in np.arange(0.05, 1.3, 0.025)]
    sims = [round(x, 3) for x in np.arange(-0.2, 0.95, 0.025)]
    for model in ["text-embedding-3-small", "text-embedding-3-large"]:
        for fmt, center in [("raw", False), ("topic", False), ("raw", True), ("topic", True)]:
            cfg = f"{model.split('-')[-1]}/{fmt}" + ("/centered" if center else "")
            per = {}
            for name in DATASETS:
                topic, claims, truth = load(name)
                texts = claims if fmt == "raw" else [f"題目：{topic}\n主張：{c}" for c in claims]
                X, toks = embed(texts, model)
                if center:
                    X = X - X.mean(axis=0)
                    X = X / np.linalg.norm(X, axis=1, keepdims=True)
                k_true = len(set(truth))
                per[name] = {
                    "tokens": toks,
                    "kmeans_sil": score(truth, kmeans_sil(X)),
                    "kmeans_oracle_k": score(truth, kmeans_k(X, k_true)),
                    "hdbscan": score(truth, hdb(X)),
                    "agglo": {t: score(truth, agglo(X, t)) for t in thrs},
                    "online": {t: score(truth, online(X, t)) for t in sims},
                }
            # 門檻：每組資料最佳（oracle）與留一組資料交叉選門檻（lodo，較接近實際）
            for algo, grid in [("agglo", thrs), ("online", sims)]:
                for name in DATASETS:
                    others = [n for n in DATASETS if n != name]
                    t_lodo = max(grid, key=lambda t: np.mean([per[n][algo][t]["ari"] for n in others]))
                    t_best = max(grid, key=lambda t: per[name][algo][t]["ari"])
                    per[name][f"{algo}_lodo"] = {**per[name][algo][t_lodo], "thr": t_lodo}
                    per[name][f"{algo}_oracle"] = {**per[name][algo][t_best], "thr": t_best}
            res[cfg] = per
    (HERE / "emb_results.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))


def stage_naming():
    """混合方案的命名成本：embedding 分完群後，請 LLM 只替每群命名（一次呼叫）。"""
    res = {}
    for name in DATASETS:
        topic, claims, truth = load(name)
        X, _ = embed([f"題目：{topic}\n主張：{c}" for c in claims], "text-embedding-3-large")
        X = X - X.mean(axis=0)
        X = X / np.linalg.norm(X, axis=1, keepdims=True)
        lab = kmeans_sil(X)
        blocks = []
        for g in sorted(set(lab)):
            members = [c for c, l in zip(claims, lab) if l == g][:6]
            blocks.append(f"群組 {g + 1}：\n" + "\n".join(f"- {m}" for m in members))
        system = (
            f"你在整理學生對討論題目「{topic}」的主張，只使用繁體中文。以下主張已經分好群（每群列出部分例句）。\n"
            "請替每一群取一個簡短的名稱（12 字內，描述主張本身）。\n"
            + "\n".join(blocks)
            + '\n只輸出 JSON：{"names":{"1":"群組名稱"}}'
        )
        raw, usage = chat(system, "請命名。", "naming")
        res[name] = {"usage": usage, "names": extract_json(raw), "score": score(truth, lab)}
    (HERE / "naming_results.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    {"llm": stage_llm, "emb": stage_emb, "naming": stage_naming}[sys.argv[2]]()
