#!/usr/bin/env python3
"""抽出 Workers 版每一條靜態 SQL（`sqlx::query*` 的字串常值、`concat!` 與 SQL 巨集），輸出成 JSON，
讓呼叫端對 PostgreSQL（套用 backend/migrations 的試行資料庫）逐條 `PREPARE` 檢查。

PREPARE 只解析與規劃、不執行：能抓到語法錯誤、不存在的表或欄位、無法推斷型別的參數。
抓不到的：執行期的型別問題、條件邏輯是否與後端相同。

用法：python3 -I scripts/check_sql.py [src 目錄] > statements.json
"""
import glob
import os
import re
import json
import sys

SRC = sys.argv[1] if len(sys.argv) > 1 else 'src'

# lib.rs 的 generation_unclaimed!() 巨集內容
MACROS = {}


def read_rust_string(src, i):
    """從 src[i] 讀一個 Rust 字串常值，回傳 (內容, 結束位置)；不是字串回傳 None。"""
    m = re.match(r'r(#*)"', src[i:])
    if m:
        hashes = m.group(1)
        start = i + len(m.group(0))
        end = src.index('"' + hashes, start)
        return src[start:end], end + 1 + len(hashes)
    if src[i] != '"':
        return None
    j = i + 1
    out = []
    while True:
        c = src[j]
        if c == '\\':
            n = src[j + 1]
            if n == '\n':
                # 行尾續行：跳過換行與下一行開頭的空白
                j += 2
                while src[j] in ' \t\n':
                    j += 1
                continue
            out.append({'n': '\n', 't': '\t', '"': '"', '\\': '\\', "'": "'", '0': '\0'}.get(n, n))
            j += 2
            continue
        if c == '"':
            return ''.join(out), j + 1
        out.append(c)
        j += 1


def skip_ws(src, i):
    while src[i] in ' \t\n':
        i += 1
    return i


def read_expr(src, i, env):
    """讀一個 SQL 片段運算式：字串常值、`$參數`、`concat!(...)` 或巨集呼叫。回傳 (字串, 結束位置) 或 None。"""
    i = skip_ws(src, i)
    lit = read_rust_string(src, i)
    if lit:
        return lit
    m = re.match(r'\$(\w+)', src[i:])
    if m and m.group(1) in env:
        return env[m.group(1)], i + len(m.group(0))
    m = re.match(r'(\w+)!\(', src[i:])
    if not m:
        return None
    name = m.group(1)
    j = i + len(m.group(0))
    args = []
    while True:
        j = skip_ws(src, j)
        if src[j] == ')':
            j += 1
            break
        item = read_expr(src, j, env)
        if item is None:
            return None
        args.append(item[0])
        j = skip_ws(src, item[1])
        if src[j] == ',':
            j += 1
    if name == 'concat':
        return ''.join(args), j
    if name not in MACROS:
        return None
    params, body = MACROS[name]
    if len(params) != len(args):
        return None
    out = read_expr(body, 0, dict(zip(params, args)))
    return (out[0], j) if out else None


def read_sql_expr(src, i):
    """讀 query(...) 的第一個參數；回傳 None 表示動態 SQL（變數、format! 等）。"""
    out = read_expr(src, i, {})
    return out[0] if out else None


def collect_macros(paths):
    """收集 `macro_rules! name { ($p:literal) => { body }; }` 形式的 SQL 巨集。"""
    for path in paths:
        src = open(path).read()
        for m in re.finditer(r'macro_rules! (\w+) \{\s*\(([^)]*)\) => \{', src):
            params = re.findall(r'\$(\w+):literal', m.group(2))
            # body 到對應的 `};` 為止（巨集本體不含 `};`）
            end = src.index('};', m.end())
            MACROS[m.group(1)] = (params, src[m.end():end].strip() + ' ')


def main():
    collect_macros(sorted(glob.glob(os.path.join(SRC, '*.rs'))))
    out = []
    dynamic = []
    pattern = re.compile(r'sqlx::query(?:_as|_scalar)?(?:::<[^(]*?>)?\(')
    for path in sorted(glob.glob(os.path.join(SRC, '*.rs'))):
        src = open(path).read()
        # 測試模組不檢查
        cut = src.find('#[cfg(test)]')
        body = src if cut < 0 else src[:cut]
        for m in pattern.finditer(body):
            line = body.count('\n', 0, m.start()) + 1
            loc = f'{os.path.basename(path)}:{line}'
            sql = read_sql_expr(body, m.end())
            if sql is None:
                dynamic.append(loc)
            else:
                out.append({'loc': loc, 'sql': sql})
    json.dump({'statements': out, 'dynamic': dynamic}, sys.stdout, ensure_ascii=False, indent=1)


if __name__ == '__main__':
    main()
