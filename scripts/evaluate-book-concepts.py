#!/usr/bin/env python3
"""Small source-search experiment, not production retrieval or an LLM evaluation."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import runpy
import time
from urllib.parse import urlencode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sample', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--provider', choices=('google', 'openlibrary'), required=True)
    parser.add_argument('--key-file', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Output exists; use a new filename to preserve evidence')
    helpers = runpy.run_path(str(Path(__file__).with_name('evaluate-book-sources.py')))
    key = None
    if args.provider == 'google' and args.key_file:
        for line in args.key_file.read_text().splitlines():
            name, sep, value = line.strip().removeprefix('export ').partition('=')
            if sep and name.strip() == 'GOOGLE_BOOKS_API_KEY':
                key = value.strip().strip('\"\'')
    sample = json.loads(args.sample.read_text())
    report = {'provider': args.provider, 'sample': args.sample.name,
              'started_at': datetime.now(timezone.utc).isoformat(),
              'key_used': bool(key), 'results': [],
              'method': 'fixed concept phrases; unfiltered / language-filtered; top 5; sequential 1.1s gap; timeout 10s; no retries; no LLM'}
    stop = 'missing_api_key' if args.provider == 'google' and not key else None
    for case in sample['cases']:
        for filtered in (False, True):
            row = {'case_id': case['id'], 'language': case['language'], 'filtered': filtered}
            if stop:
                row.update(status='skipped', reason=stop)
            else:
                if args.provider == 'google':
                    params = {'q': case['query'], 'maxResults': 5, 'printType': 'books'}
                    if filtered:
                        params['langRestrict'] = case['google_language']
                    url = 'https://www.googleapis.com/books/v1/volumes?' + urlencode(params)
                else:
                    query = case['query']
                    if filtered:
                        query += ' language:' + case['openlibrary_language']
                    params = {'q': query, 'limit': 5,
                              'fields': 'key,title,author_name,language,isbn,editions,editions.key,editions.title,editions.language'}
                    url = 'https://openlibrary.org/search.json?' + urlencode(params)
                status, payload = helpers['fetch'](url, key if args.provider == 'google' else None)
                row.update(status, request_url=url)
                if payload is not None:
                    row['candidates'] = helpers['candidates'](args.provider, 'title', payload)
                if row.get('http') in (401, 403, 429):
                    stop = 'provider_access_or_quota_error'
                time.sleep(1.1)
            report['results'].append(row)
            args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
            print(case['id'], filtered, row['status'], len(row.get('candidates', [])), flush=True)
    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    main()
