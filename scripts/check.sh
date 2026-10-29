#!/usr/bin/env bash
# Runs every check that is run before a merge, in order, stopping at the first failure.
# Run `scripts/check.sh --help` for the options.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/check.sh [--port N] [--firefox] [--firefox-strict] [--no-e2e] [--help]

Runs, in order, stopping at the first failure:
  cargo build, cargo test, cargo clippy --all-targets -D warnings, cargo fmt --check,
  cargo sqlx prepare --check, npm run prod (from a deleted
  assets/css/main.css), npm run typecheck, npm run knip, npm run test:unit, npm run test (e2e)

Options:
  --port N          e2e server port (FP_E2E_PORT; default: the harness default, 8081)
  --firefox         also run the e2e suite in Firefox (FP_E2E_FIREFOX=1, --project=firefox).
                    Known Firefox failures are reported but don't fail the script
  --firefox-strict  like --firefox, but a Firefox failure makes the script fail too
  --no-e2e          skip both e2e steps
  --help            show this help

Database: DATABASE_URL is never taken from a .env file. If it isn't set, a throwaway database
<cargo target dir>/fp-check.sqlite is created and migrated, and used by every cargo step and by
the e2e harness's build (so cargo doesn't rebuild because of a different DATABASE_URL).
Works from the main checkout and from worktrees (git worktree) under .claude/worktrees/.
USAGE
}

port=""
firefox=0
firefox_strict=0
e2e=1
while [[ $# -gt 0 ]]; do
    case "$1" in
        --port) port="${2:?--port needs a value}"; shift 2 ;;
        --firefox) firefox=1; shift ;;
        --firefox-strict) firefox=1; firefox_strict=1; shift ;;
        --no-e2e) e2e=0; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."
[[ -n "$port" ]] && export FP_E2E_PORT="$port"

log_dir="$(mktemp -d "${TMPDIR:-/tmp}/fp-check.XXXXXX")"

# ---- Database -------------------------------------------------------------------------------
if [[ -z "${DATABASE_URL:-}" ]]; then
    target_dir="$(cargo metadata --format-version 1 --no-deps \
        | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"
    check_db="$target_dir/fp-check.sqlite"
    rm -f "$check_db" "$check_db-shm" "$check_db-wal"
    export DATABASE_URL="sqlite:$check_db"
    sqlx database create
    sqlx migrate run
fi
echo "DATABASE_URL=$DATABASE_URL"

# ---- Steps ----------------------------------------------------------------------------------
names=()
results=()
failed_step=""
rust_tests="-"
unit_tests="-"
e2e_counts="-"
firefox_counts="-"

record() { names+=("$1"); results+=("$2"); }

# run_step <name> <command...>: output goes to a log; shown if the step fails
run_step() {
    local name="$1"; shift
    local log="$log_dir/${name// /_}.log"
    if [[ -n "$failed_step" ]]; then
        record "$name" "skipped"
        return 0
    fi
    echo "==> $name"
    if "$@" >"$log" 2>&1; then
        record "$name" "ok"
    else
        record "$name" "FAILED"
        failed_step="$name"
        echo "FAILED: $name ($*). Last output (full log: $log):" >&2
        tail -n 40 "$log" >&2
    fi
}

skip_step() { record "$1" "skipped${2:+ ($2)}"; }

# Sums the "N <word>" counts of a Playwright/cargo log
count_of() { grep -Eo "[0-9]+ $2" "$1" | awk '{s+=$1} END {print s+0}'; }
playwright_counts() {
    echo "$(count_of "$1" passed) passed, $(count_of "$1" skipped) skipped, $(count_of "$1" failed) failed"
}

run_step "cargo build" cargo build
run_step "cargo test" cargo test
[[ -f "$log_dir/cargo_test.log" ]] && rust_tests="$(grep -E '^test result:' "$log_dir/cargo_test.log" | grep -Eo '[0-9]+ passed' | awk '{s+=$1} END {print s+0}') passed"
run_step "cargo clippy" cargo clippy --all-targets -- -D warnings
run_step "cargo fmt" cargo fmt --check
run_step "sqlx prepare --check" cargo sqlx prepare --check
run_step "npm run prod" bash -c 'rm -f assets/css/main.css && npm run prod'
run_step "npm run typecheck" npm run typecheck
run_step "npm run knip" npm run knip
run_step "npm run test:unit" npm run test:unit
[[ -f "$log_dir/npm_run_test:unit.log" ]] && unit_tests="$(playwright_counts "$log_dir/npm_run_test:unit.log")"

if [[ $e2e -eq 1 ]]; then
    run_step "npm run test (e2e)" npm run test
    [[ -f "$log_dir/npm_run_test_(e2e).log" ]] && e2e_counts="$(playwright_counts "$log_dir/npm_run_test_(e2e).log")"
else
    skip_step "npm run test (e2e)" "--no-e2e"
fi

firefox_failed=0
if [[ $e2e -eq 1 && $firefox -eq 1 ]]; then
    if [[ -z "$failed_step" ]]; then
        echo "==> e2e in Firefox"
        ff_log="$log_dir/firefox.log"
        if FP_E2E_FIREFOX=1 npm run test -- --project=firefox >"$ff_log" 2>&1; then
            record "e2e in Firefox" "ok"
        else
            firefox_failed=1
            if [[ $firefox_strict -eq 1 ]]; then
                record "e2e in Firefox" "FAILED"
                failed_step="e2e in Firefox"
            else
                record "e2e in Firefox" "FAILED (known issues, not fatal)"
            fi
        fi
        firefox_counts="$(playwright_counts "$ff_log")"
        grep -E '^\s+[0-9]+\) ' "$ff_log" | sort -u | head -n 40 >"$log_dir/firefox-failures.txt" || true
    else
        skip_step "e2e in Firefox"
    fi
else
    skip_step "e2e in Firefox" "$([[ $e2e -eq 0 ]] && echo '--no-e2e' || echo 'no --firefox')"
fi

# ---- Summary --------------------------------------------------------------------------------
echo
echo "================ Summary ================"
for i in "${!names[@]}"; do
    printf '  %-24s %s\n' "${names[$i]}" "${results[$i]}"
done
echo "-----------------------------------------"
echo "  Rust tests:      $rust_tests"
echo "  Unit tests (JS): $unit_tests"
echo "  e2e:             $e2e_counts"
[[ $firefox -eq 1 ]] && echo "  e2e (Firefox):   $firefox_counts"
if [[ $firefox_failed -eq 1 && -s "$log_dir/firefox-failures.txt" ]]; then
    echo "  Firefox failures:"
    sed 's/^/    /' "$log_dir/firefox-failures.txt"
fi
echo "  Logs:            $log_dir"
if [[ -n "$failed_step" ]]; then
    echo "RESULT: FAILED at '$failed_step'"
    exit 1
fi
echo "RESULT: ok"
