"""把 docker/cascade/launch.yaml 轉成 macOS 原生執行用的設定，不另外維護一份。

只換三種東西：compose 服務名（轉接層、後端）換成本機位址，`model_path` 換成執行目錄。
找不到預期的內容就失敗（launch.yaml 改過格式時要重新檢視這裡），不會默默略過。
用法：python3 render_launch.py SRC DST --app-url http://127.0.0.1:3000 --model-path DIR
"""
import argparse
import pathlib

ADAPTER = 'http://cascade-adapter:8103'
APP = 'http://app:3000'


def _replace(text, old, new, expected, label):
    found = text.count(old)
    if found != expected:
        raise ValueError(f'launch.yaml: expected {expected} x {label} ({old!r}), found {found}')
    return text.replace(old, new)


def render(text, app_url, model_path, adapter_url='http://127.0.0.1:8103'):
    text = _replace(text, ADAPTER, adapter_url.rstrip('/'), 2, 'cascade-adapter URL')
    text = _replace(text, APP, app_url.rstrip('/'), 1, 'app URL')
    return _replace(text, 'model_path: /app\n', f'model_path: {model_path}\n', 1, 'model_path')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('src')
    parser.add_argument('dst')
    parser.add_argument('--app-url', default='http://127.0.0.1:3000')
    parser.add_argument('--adapter-url', default='http://127.0.0.1:8103')
    parser.add_argument('--model-path', required=True)
    args = parser.parse_args()
    out = render(pathlib.Path(args.src).read_text(), args.app_url, args.model_path, args.adapter_url)
    pathlib.Path(args.dst).write_text(out)


if __name__ == '__main__':
    main()
