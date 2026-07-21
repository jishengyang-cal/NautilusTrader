#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Rust formatting hook tests"
  exit 0
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_formatting_rs.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_rs() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

create_case() {
  local case_dir="$1"

  mkdir -p "$case_dir"/{crates/common/src,tests,examples,docs}
}

run_hook() {
  local case_dir="$1"

  (cd "$case_dir" && bash "$HOOK") > "$case_dir/output.txt" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local pattern="$2"

  if run_hook "$case_dir"; then
    echo "Expected Rust formatting hook to fail in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi

  rg -q "$pattern" "$case_dir/output.txt"
}

match_guard_and_if_case="$CASE_ROOT/allow-match-guard-reject-missing-blank"
create_case "$match_guard_and_if_case"
write_rs "$match_guard_and_if_case/crates/common/src/lib.rs" \
  'pub fn map_status(status: Status, filled_qty: Quantity, reason: &str) -> Status {' \
  '    match status {' \
  '        Status::Canceled' \
  '            if filled_qty.is_zero()' \
  '                && due_post_only(reason) =>' \
  '        {' \
  '            Status::Rejected' \
  '        }' \
  '        status => status,' \
  '    }' \
  '}' \
  '' \
  'pub fn check_ready(ready: bool, enabled: bool) {' \
  '    prepare();' \
  '    if ready' \
  '        && enabled' \
  '    {' \
  '        run();' \
  '    }' \
  '}'
expect_failure "$match_guard_and_if_case" "crates/common/src/lib.rs:15"

violation_count=$(rg -c "Missing blank line above" "$match_guard_and_if_case/output.txt")
if [ "$violation_count" -ne 1 ]; then
  echo "Expected exactly one missing-blank violation"
  cat "$match_guard_and_if_case/output.txt"
  exit 1
fi

echo "Rust formatting hook tests passed"
