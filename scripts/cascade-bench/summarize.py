"""彙整 scn.py 的輸出。用法：python3 summarize.py 名稱=檔案 [名稱=檔案 ...]

turn／pause 情境：印「辨識（含 Smart Turn 等待）」「辨識後到首段音訊」「speech_stopped 到首段音訊」的中位數與 p90。
這三個間隔都以事件互相相減，不受測試音訊與 100 ms 幀對齊的影響；speech_stopped_s 本身會因對齊落在兩個值之一，不拿來比較。
interrupt 情境：印偵測延遲、取消延遲與取消前多送的音訊包數。
"""
import json
import statistics as st
import sys


def rows(path):
    out = []
    for line in open(path):
        if line.startswith('RES '):
            try:
                out.append(json.loads(line[4:]))
            except ValueError:
                pass
    return out


def q(values, p):
    values = sorted(values)
    return values[min(len(values) - 1, int(round(p * (len(values) - 1))))]


def stat(values):
    return f'med {st.median(values):.2f} p90 {q(values, .9):.2f}' if values else 'n/a'


for arg in sys.argv[1:]:
    name, path = arg.split('=', 1)
    r = rows(path)
    if r and r[0].get('scen') == 'interrupt':
        print(f'{name} interrupt n={len(r)}')
        for key in ('int_started_s', 'cancel_after_started_s', 'leftover_audio_chunks'):
            print(f'   {key}: ' + stat([x[key] for x in r if x.get(key) is not None]))
        continue
    if r and r[0].get('scen') == 'pause':
        # 被切成兩句時，第一個轉錄早於最後的 speech_stopped，間隔沒有意義，只看轉錄次數
        print(f'{name} pause n={len(r)} 轉錄次數（1 表示沒被切成兩句）: ' + str([x.get('transcripts') for x in r]))
        continue
    ok = [x for x in r if x.get('first_audio_s') and x.get('transcript_s') and x.get('stopped_s') is not None]
    print(f'{name} n={len(ok)} 失敗={len(r) - len(ok)}')
    print('   辨識（含 Smart Turn 等待）: ' + stat([x['transcript_s'] - x['stopped_s'] for x in ok]))
    print('   辨識後到首段音訊:          ' + stat([x['first_audio_s'] - x['transcript_s'] for x in ok]))
    print('   speech_stopped 到首段音訊: ' + stat([x['first_audio_s'] - x['stopped_s'] for x in ok]))
