"""Offline F-07 response-vs-label review. No OpenAI calls.
Usage: python audit_f07_offline.py "C:\\Project\\socrates-chat\\backend\\eval\\results\\<run_id>"
"""
import argparse
import csv
import json
from pathlib import Path

TYPES = {'clarify','reason','assumption','counterexample','perspective','wrap_up'}

def analyze(record):
    raw = record.get('raw_output')
    if not isinstance(raw, str):
        return '', '', 'API_ERROR', 'Missing raw output'
    if '<<<META>>>' not in raw:
        return raw.strip(), '', 'FORMAT_ERROR', 'Missing <<<META>>> delimiter'
    response, metadata = raw.split('<<<META>>>', 1)
    try:
        parsed = json.loads(metadata.strip())
    except json.JSONDecodeError as e:
        return response.strip(), '', 'FORMAT_ERROR', f'Invalid JSON: {e}'
    if not isinstance(parsed, dict):
        return response.strip(), '', 'FORMAT_ERROR', 'META must be JSON object'
    actual = parsed.get('question_type')
    if actual not in TYPES:
        return response.strip(), str(actual), 'FORMAT_ERROR', 'Invalid question_type'
    if type(parsed.get('advance')) is not bool:
        return response.strip(), actual, 'FORMAT_ERROR', 'advance must be JSON boolean'
    if not isinstance(parsed.get('reason'), str) or not parsed['reason'].strip():
        return response.strip(), actual, 'FORMAT_ERROR', 'Missing nonempty reason'
    return response.strip(), actual, 'VALID', ''

def main():
    p = argparse.ArgumentParser()
    p.add_argument('run_directory', type=Path)
    args = p.parse_args()
    files = sorted(args.run_directory.glob('[0-9][0-9].json'))
    if not files:
        p.error('No NN.json result files found in that directory')
    rows=[]
    for f in files:
        item=json.loads(f.read_text(encoding='utf-8-sig'))
        response, actual, fmt, error=analyze(item)
        rows.append({
          'case_id':item.get('case_id',''),
          'expected_type':item.get('expected_type',''),
          'declared_type':actual,
          'format_check':fmt,
          'format_issue':error,
          'actual_question':response,
          'actual_question_type_MANUAL':'',
          'label_consistency_MANUAL':'NOT_REVIEWED',
          'teaching_quality_MANUAL':'NOT_REVIEWED',
          'review_notes':''
        })
        print(f"{item.get('case_id','?')} | format={fmt} | declared={actual or '-'} | {error}\n  Question: {response[:220]}\n")
    out=args.run_directory/'manual_audit.csv'
    with out.open('w',newline='',encoding='utf-8-sig') as f:
        writer=csv.DictWriter(f,fieldnames=rows[0].keys())
        writer.writeheader(); writer.writerows(rows)
    print(f'Wrote {out} | {len(rows)} cases. Label consistency awaits manual judgment.')

if __name__=='__main__':
    main()
