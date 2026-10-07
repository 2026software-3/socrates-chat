#!/usr/bin/env bash
# Keeps the Project "Status" in sync with GitHub's native blocked-by relations:
#   Backlog -> Ready    when an open Task has no open blocked-by left
#   Ready   -> Backlog  when an open Task has an open blocked-by again
# Only Issue Type "Task" is touched; In progress / In review / Done are never changed.
#
# Env: GH_TOKEN (needs org Projects read/write), REPO (owner/name), ORG, PROJECT_NUMBER,
#      MODE (event|sweep), ISSUE (event mode: the issue that was closed/reopened), DRY_RUN (true|false)
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

set_status() { # item_id option_id
  gh api graphql -f project="$PROJECT_ID" -f item="$1" -f field="$FIELD_ID" -f opt="$2" -f query='
    mutation($project: ID!, $item: ID!, $field: ID!, $opt: String!) {
      updateProjectV2ItemFieldValue(input: {projectId: $project, itemId: $item, fieldId: $field,
        value: {singleSelectOptionId: $opt}}) { projectV2Item { id } }
    }' >/dev/null
}

reconcile() { # issue number
  local n="$1" info state type item status open
  info=$(gh api graphql -f owner="$OWNER" -f name="$NAME" -F n="$n" -f query='
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
  state=$(jq -r '.data.repository.issue.state' <<<"$info")
  type=$(jq -r '.data.repository.issue.issueType.name // ""' <<<"$info")
  item=$(jq -r --arg p "$PROJECT_ID" '[.data.repository.issue.projectItems.nodes[] | select(.project.id == $p)][0].id // ""' <<<"$info")
  status=$(jq -r --arg p "$PROJECT_ID" '[.data.repository.issue.projectItems.nodes[] | select(.project.id == $p)][0].fieldValueByName.name // ""' <<<"$info")
  [[ "$state" == OPEN && "$type" == Task && -n "$item" ]] || return 0
  [[ "$status" == Backlog || "$status" == Ready ]] || return 0

  open=$(gh api "repos/$REPO/issues/$n/dependencies/blocked_by" --paginate --jq '.[] | select(.state == "open") | .number' | wc -l)
  if [[ "$status" == Backlog && "$open" -eq 0 ]]; then
    echo "#$n: Backlog -> Ready"
    [[ "$DRY_RUN" == true ]] || set_status "$item" "$READY_ID"
  elif [[ "$status" == Ready && "$open" -gt 0 ]]; then
    echo "#$n: Ready -> Backlog ($open open blocker(s))"
    [[ "$DRY_RUN" == true ]] || set_status "$item" "$BACKLOG_ID"
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
  *) echo "unknown MODE: $MODE" >&2; exit 1 ;;
esac
