#!/usr/bin/env bash
# Tests project-status.sh against a fake `gh`, including transient and permanent API failures.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir "$TMP/bin"

# Fake gh. It answers by looking for a marker in its arguments.
#   FAKE_ISSUE_FAILS=n   the first n issue queries return a non-JSON body (-1: always)
#   FAKE_MUTATION_FAILS=n  same for the status mutation
#   FAKE_STATUS          Status of issue #7 on the Project (default Backlog)
cat >"$TMP/bin/gh" <<'FAKE'
#!/usr/bin/env bash
args="$*"
count() { # counter name, failures allowed (-1 = always) -> exit 0 when this call should fail
  local f="$FAKE_DIR/$1" n=0
  [[ -f "$f" ]] && n=$(<"$f")
  echo $((n + 1)) >"$f"
  [[ "$2" -lt 0 || "$n" -lt "$2" ]]
}
case "$args" in
  *"projectV2(number"*)
    echo '{"data":{"organization":{"projectV2":{"id":"P1","field":{"id":"F1","options":[
      {"id":"o-backlog","name":"Backlog"},{"id":"o-ready","name":"Ready"},
      {"id":"o-progress","name":"In progress"},{"id":"o-review","name":"In review"},{"id":"o-done","name":"Done"}]}}}}}' ;;
  *"issue(number"*)
    if count issue "${FAKE_ISSUE_FAILS:-0}"; then echo "HTTP 502 Bad Gateway"; exit 0; fi
    echo '{"data":{"repository":{"issue":{"state":"OPEN","issueType":{"name":"Task"},"projectItems":{"nodes":[
      {"id":"I1","project":{"id":"P1"},"fieldValueByName":{"name":"'"${FAKE_STATUS:-Backlog}"'"}}]}}}}}' ;;
  *"updateProjectV2ItemFieldValue"*)
    if count mutation "${FAKE_MUTATION_FAILS:-0}"; then echo '{"errors":[{"message":"boom"}]}'; exit 0; fi
    echo "$args" | grep -o 'opt=[^ ]*' >>"$FAKE_DIR/mutations"
    echo '{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"I1"}}}}' ;;
  *) echo "unexpected gh call: $args" >&2; exit 1 ;;
esac
FAKE
chmod +x "$TMP/bin/gh"

failures=0
run() { # name, expected exit, expected output regex, expected mutation count, env...
  local name="$1" want_exit="$2" want_out="$3" want_mut="$4" out code muts=0
  shift 4
  rm -f "$TMP"/issue "$TMP"/mutation "$TMP"/mutations
  out=$(env PATH="$TMP/bin:$PATH" FAKE_DIR="$TMP" GH_TOKEN=x REPO=o/r ORG=o PROJECT_NUMBER=1 \
    MODE=assigned ISSUE=7 RETRY_DELAY=0 DRY_RUN=false "$@" "$HERE/project-status.sh" 2>&1)
  code=$?
  [[ -f "$TMP/mutations" ]] && muts=$(wc -l <"$TMP/mutations")
  if [[ "$code" -eq "$want_exit" && "$out" =~ $want_out && "$muts" -eq "$want_mut" ]]; then
    echo "ok   - $name"
  else
    echo "FAIL - $name (exit $code, mutations $muts)"
    echo "$out" | sed 's/^/       /'
    failures=$((failures + 1))
  fi
}

run "moves a Backlog issue on assignment" 0 '#7: Backlog -> In progress' 1
run "does not touch an issue already in review" 0 '^$' 0 FAKE_STATUS="In review"
run "dry run does not write" 0 '#7: Backlog -> In progress' 0 DRY_RUN=true
run "retries a transient non-JSON response" 0 'attempt 2/4 failed.*#7: Backlog -> In progress' 1 FAKE_ISSUE_FAILS=2
run "retries a transient GraphQL error on write" 0 'attempt 1/4 failed' 1 FAKE_MUTATION_FAILS=1
run "reports a permanent read failure" 1 '::error::#7: could not read the issue' 0 FAKE_ISSUE_FAILS=-1
run "reports a permanent write failure" 1 '::error::#7: could not set Status to In progress' 0 FAKE_MUTATION_FAILS=-1

[[ "$failures" -eq 0 ]] && echo "all passed" || { echo "$failures failed"; exit 1; }
