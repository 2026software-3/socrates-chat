#!/usr/bin/env bash
# Keeps the Project "Status" of open Task issues in sync:
#   blocked-by (MODE=event|sweep)
#     Backlog -> Ready        when no open blocked-by is left
#     Ready   -> Backlog      when an open blocked-by appears again
#   assignment (MODE=assigned)
#     Backlog|Ready -> In progress
#   pull requests (MODE=pr), for the issues the PR closes via "Closes #n"
#     opened / reopened / ready for review (not draft): Backlog|Ready|In progress -> In review
#     converted to draft / closed without merge:         In review -> In progress
# Only Issue Type "Task" is touched. Done is set by the Project's built-in workflow, never here.
#
# Env: GH_TOKEN (needs org Projects read/write), REPO (owner/name), ORG, PROJECT_NUMBER,
#      MODE (event|sweep|assigned|pr), DRY_RUN (true|false),
#      ISSUE (event, assigned), PR / PR_ACTION / PR_DRAFT / PR_MERGED (pr),
#      READ_TOKEN (pr: optional token that can read pull requests, defaults to GH_TOKEN)
set -euo pipefail

: "${GH_TOKEN:?PROJECT_TOKEN secret is not set; GITHUB_TOKEN cannot write org Projects (see CONTRIBUTING.md)}"
: "${REPO:?}" "${ORG:?}" "${PROJECT_NUMBER:?}" "${MODE:?}"
DRY_RUN="${DRY_RUN:-false}"
OWNER="${REPO%%/*}"
NAME="${REPO##*/}"

meta=$(gh api graphql -f org="$ORG" -F num="$PROJECT_NUMBER" -f query='
  query($org: String!, $num: Int!) {
    organization(login: $org) {
      projectV2(number: $num) {
        id
        field(name: "Status") { ... on ProjectV2SingleSelectField { id options { id name } } }
      }
    }
  }')
PROJECT_ID=$(jq -r '.data.organization.projectV2.id' <<<"$meta")
FIELD_ID=$(jq -r '.data.organization.projectV2.field.id' <<<"$meta")
option_id() { jq -er --arg n "$1" '.data.organization.projectV2.field.options[] | select(.name == $n) | .id' <<<"$meta"; }
READY_ID=$(option_id Ready)
BACKLOG_ID=$(option_id Backlog)
INPROGRESS_ID=$(option_id "In progress")
INREVIEW_ID=$(option_id "In review")

set_status() { # item_id option_id
  gh api graphql -f project="$PROJECT_ID" -f item="$1" -f field="$FIELD_ID" -f opt="$2" -f query='
    mutation($project: ID!, $item: ID!, $field: ID!, $opt: String!) {
      updateProjectV2ItemFieldValue(input: {projectId: $project, itemId: $item, fieldId: $field,
        value: {singleSelectOptionId: $opt}}) { projectV2Item { id } }
    }' >/dev/null
}

# Sets STATE TYPE ITEM STATUS for an issue; returns 1 unless it is an open Task on the Project.
load_issue() { # issue number
  local info
  info=$(gh api graphql -f owner="$OWNER" -f name="$NAME" -F n="$1" -f query='
    query($owner: String!, $name: String!, $n: Int!) {
      repository(owner: $owner, name: $name) {
        issue(number: $n) {
          state
          issueType { name }
          projectItems(first: 20) {
            nodes {
              id
              project { id }
              fieldValueByName(name: "Status") { ... on ProjectV2ItemFieldSingleSelectValue { name } }
            }
          }
        }
      }
    }')
  STATE=$(jq -r '.data.repository.issue.state' <<<"$info")
  TYPE=$(jq -r '.data.repository.issue.issueType.name // ""' <<<"$info")
  ITEM=$(jq -r --arg p "$PROJECT_ID" '[.data.repository.issue.projectItems.nodes[] | select(.project.id == $p)][0].id // ""' <<<"$info")
  STATUS=$(jq -r --arg p "$PROJECT_ID" '[.data.repository.issue.projectItems.nodes[] | select(.project.id == $p)][0].fieldValueByName.name // ""' <<<"$info")
  [[ "$STATE" == OPEN && "$TYPE" == Task && -n "$ITEM" ]]
}

move() { # issue number, to-status name, to-option id, then the statuses it may move from
  local n="$1" to="$2" opt="$3" from
  shift 3
  for from in "$@"; do
    if [[ "$STATUS" == "$from" ]]; then
      echo "#$n: $STATUS -> $to"
      [[ "$DRY_RUN" == true ]] || set_status "$ITEM" "$opt"
      return 0
    fi
  done
}

reconcile() { # issue number
  local n="$1" open
  load_issue "$n" || return 0
  [[ "$STATUS" == Backlog || "$STATUS" == Ready ]] || return 0
  open=$(gh api "repos/$REPO/issues/$n/dependencies/blocked_by" --paginate --jq '.[] | select(.state == "open") | .number' | wc -l)
  if [[ "$open" -eq 0 ]]; then
    move "$n" Ready "$READY_ID" Backlog
  else
    move "$n" Backlog "$BACKLOG_ID" Ready
  fi
}

case "$MODE" in
  event)
    : "${ISSUE:?}"
    for n in $(gh api "repos/$REPO/issues/$ISSUE/dependencies/blocking" --paginate --jq '.[].number'); do
      reconcile "$n"
    done
    ;;
  sweep)
    for n in $(gh api "repos/$REPO/issues?state=open&per_page=100" --paginate --jq '.[] | select(.pull_request | not) | .number'); do
      reconcile "$n"
    done
    ;;
  assigned)
    : "${ISSUE:?}"
    if load_issue "$ISSUE"; then move "$ISSUE" "In progress" "$INPROGRESS_ID" Backlog Ready; fi
    ;;
  pr)
    : "${PR:?}" "${PR_ACTION:?}"
    case "$PR_ACTION" in
      opened | reopened | ready_for_review)
        [[ "${PR_DRAFT:-false}" == true ]] && exit 0
        to="In review"; opt="$INREVIEW_ID"; from=(Backlog Ready "In progress")
        ;;
      converted_to_draft | closed)
        [[ "$PR_ACTION" == closed && "${PR_MERGED:-false}" == true ]] && exit 0 # merged: built-in workflow sets Done
        to="In progress"; opt="$INPROGRESS_ID"; from=("In review")
        ;;
      *) exit 0 ;;
    esac
    for n in $(GH_TOKEN="${READ_TOKEN:-$GH_TOKEN}" gh api graphql -f owner="$OWNER" -f name="$NAME" -F pr="$PR" -f query='
      query($owner: String!, $name: String!, $pr: Int!) {
        repository(owner: $owner, name: $name) {
          pullRequest(number: $pr) { closingIssuesReferences(first: 50) { nodes { number } } }
        }
      }' --jq '.data.repository.pullRequest.closingIssuesReferences.nodes[].number'); do
      if load_issue "$n"; then move "$n" "$to" "$opt" "${from[@]}"; fi
    done
    ;;
  *) echo "unknown MODE: $MODE" >&2; exit 1 ;;
esac
