#!/usr/bin/env python3
"""Small, sequential source evaluation; not a production book importer."""
import argparse
import json
import os
from pathlib import Path
import time
from datetime import datetime, timezone
import urllib.error
import urllib.parse
import urllib.request


def fetch(url, api_key=None):
    headers = {"User-Agent": "socrates-chat-source-evaluation/0.1 (https://github.com/2026software-3/socrates-chat)"}
    if api_key:
        headers["X-Goog-Api-Key"] = api_key
    started = time.monotonic()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=10) as response:
            raw = response.read(2_000_001)
            if len(raw) > 2_000_000:
                return {"status": "response_too_large"}, None
            return {"status": "ok", "http": response.status,
                    "seconds": round(time.monotonic() - started, 3)}, json.loads(raw)
    except urllib.error.HTTPError as error:
        # Do not persist error bodies, URLs with credentials, or project identifiers.
        return {"status": "not_found" if error.code == 404 else "http_error",
                "http": error.code, "seconds": round(time.monotonic() - started, 3)}, None
    except (OSError, ValueError) as error:
        return {"status": "request_error", "error_type": type(error).__name__,
                "seconds": round(time.monotonic() - started, 3)}, None


def valid_isbn(value):
    return (len(value) == 13 and value.isdigit() and value[:3] in ("978", "979")
            and sum(int(c) * (1 if i % 2 == 0 else 3) for i, c in enumerate(value)) % 10 == 0)


def url_for(provider, book, mode):
    if provider == "google":
        query = ("isbn:" + book["isbn"] if mode == "isbn" else
                 'intitle:"' + book["query_title"] + '"')
        if mode == "title_author":
            query += ' inauthor:"' + book["author"] + '"'
        return "https://www.googleapis.com/books/v1/volumes?" + urllib.parse.urlencode(
            {"q": query, "maxResults": 5, "printType": "books"})
    if mode == "isbn":
        return "https://openlibrary.org/isbn/" + book["isbn"] + ".json"
    params = {"title": book["query_title"], "limit": 5,
              "fields": "key,title,author_name,isbn,language,first_publish_year,edition_key"}
    if mode == "title_author":
        params["author"] = book["author"]
    return "https://openlibrary.org/search.json?" + urllib.parse.urlencode(params)


def candidates(provider, mode, payload):
    if provider == "google":
        output = []
        for item in payload.get("items", []):
            v = item.get("volumeInfo", {})
            output.append({"id": item.get("id"), "title": v.get("title"),
                           "authors": v.get("authors", []), "published": v.get("publishedDate"),
                           "language": v.get("language"), "publisher": v.get("publisher"),
                           "identifiers": v.get("industryIdentifiers", []),
                           "url": v.get("infoLink"), "has_description": bool(v.get("description"))})
        return output
    if mode == "isbn":
        return [{"id": payload.get("key"), "title": payload.get("title"),
                 "author_refs": payload.get("authors", []),
                 "published": payload.get("publish_date"), "publishers": payload.get("publishers", []),
                 "languages": payload.get("languages", []), "isbn_13": payload.get("isbn_13", []),
                 "work_refs": payload.get("works", []),
                 "has_description": bool(payload.get("description"))}]
    # Search fields are work aggregates, not verified metadata for a single edition.
    return payload.get("docs", [])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sample", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--provider", choices=["google", "openlibrary"], required=True)
    parser.add_argument("--key-file", type=Path, help="Read only GOOGLE_BOOKS_API_KEY from a dotenv file; never execute it")
    args = parser.parse_args()
    key = os.environ.get("GOOGLE_BOOKS_API_KEY")
    if args.provider == "google" and args.key_file and not key:
        for line in args.key_file.read_text().splitlines():
            name, sep, value = line.strip().removeprefix("export ").partition("=")
            if sep and name.strip() == "GOOGLE_BOOKS_API_KEY":
                key = value.strip().strip("\"'")
    sample = json.loads(args.sample.read_text())
    results = []
    stop = "missing_api_key" if args.provider == "google" and not key else None
    report = {"started_at": datetime.now(timezone.utc).isoformat(),
              "provider": args.provider, "sample": args.sample.name,
              "key_used": bool(key) if args.provider == "google" else False,
              "method": "short title / short title+author / valid ISBN; top 5; 10s request timeout; sequential >=1.1s gap; no retries",
              "results": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for book in sample["books"]:
        for mode in ("title", "title_author", "isbn"):
            row = {"position": book["position"], "mode": mode}
            if mode == "isbn" and not valid_isbn(book["isbn"]):
                row.update(status="skipped", reason="no_valid_isbn")
            elif stop:
                row.update(status="skipped", reason=stop)
            else:
                url = url_for(args.provider, book, mode)
                status, payload = fetch(url, key if args.provider == "google" else None)
                row.update(status, request_url=url)
                if payload is not None:
                    row["candidates"] = candidates(args.provider, mode, payload)
                    row["total_reported"] = payload.get("totalItems", payload.get("numFound"))
                if row.get("http") in (401, 403, 429):
                    stop = "provider_access_or_quota_error"
                time.sleep(1.1)
            results.append(row)
            args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
            print(book["position"], mode, row["status"], len(row.get("candidates", [])), flush=True)
    report["finished_at"] = datetime.now(timezone.utc).isoformat()
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
