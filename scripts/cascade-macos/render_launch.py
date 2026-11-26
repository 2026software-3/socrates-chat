"""把 docker/cascade/launch.yaml 轉成 macOS 原生執行用的設定，不另外維護一份。

只換三種東西：compose 服務名（轉接層、後端）換成本機位址，`model_path` 換成執行目錄。
找不到預期的內容就失敗（launch.yaml 改過格式時要重新檢視這裡），不會默默略過。
用法：python3 render_launch.py SRC DST --app-url http://127.0.0.1:3000 --model-path DIR

回呼位址有兩種指定方式（S-19）：`--app-url` 只換主機與埠，路徑沿用 launch.yaml；
`--llm-base-url`（預設讀環境變數 `CASCADE_LLM_BASE_URL`，與 docker/cascade/entrypoint.sh 同名同義）
整串換掉，用於後端設了 `INTERNAL_LISTEN_ADDR`、內部端點在另一個埠或另一台主機時。空字串視為沒設。
"""
import argparse
import os
import pathlib

ADAPTER = 'http://cascade-adapter:8103'
APP = 'http://app:3000'
# launch.yaml 裡 LLM 節點的完整回呼位址；與 docker/cascade/entrypoint.sh 比對的字串相同
CALLBACK = f'{APP}/internal/cascade/v1'


def _replace(text, old, new, expected, label):
    found = text.count(old)
    if found != expected:
        raise ValueError(f'launch.yaml: expected {expected} x {label} ({old!r}), found {found}')
    return text.replace(old, new)


def render(text, app_url, model_path, adapter_url='http://127.0.0.1:8103', llm_base_url=None):
    text = _replace(text, ADAPTER, adapter_url.rstrip('/'), 2, 'cascade-adapter URL')
    if llm_base_url and llm_base_url.strip():
        text = _replace(text, CALLBACK, llm_base_url.strip().rstrip('/'), 1, 'callback URL')
    else:
        text = _replace(text, APP, app_url.rstrip('/'), 1, 'app URL')
    return _replace(text, 'model_path: /app\n', f'model_path: {model_path}\n', 1, 'model_path')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('src')
    parser.add_argument('dst')
    parser.add_argument('--app-url', default='http://127.0.0.1:3000')
    parser.add_argument('--adapter-url', default='http://127.0.0.1:8103')
    parser.add_argument('--llm-base-url', default=os.environ.get('CASCADE_LLM_BASE_URL', ''))
    parser.add_argument('--model-path', required=True)
    args = parser.parse_args()
    out = render(pathlib.Path(args.src).read_text(), args.app_url, args.model_path, args.adapter_url,
                 args.llm_base_url)
    pathlib.Path(args.dst).write_text(out)


if __name__ == '__main__':
    main()
