#!/usr/bin/env bash
# Tests for scripts/check-noreply.sh. Run: bash scripts/test-check-noreply.sh
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
check="$here/check-noreply.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

OK="212206047+synthalorian@users.noreply.github.com"
BOT_EMAIL="49699333+dependabot[bot]@users.noreply.github.com"
fails=0

git init -q "$tmp/repo"
cd "$tmp/repo"
git config commit.gpgsign false

# commit <author name> <author email> <committer name> <committer email> <msg>
commit() {
  GIT_AUTHOR_NAME="$1" GIT_AUTHOR_EMAIL="$2" GIT_COMMITTER_NAME="$3" GIT_COMMITTER_EMAIL="$4" \
    git commit -q --allow-empty -m "$5"
}
human()   { commit synthalorian "$OK" synthalorian "$OK" "$1"; }
dependabot() { commit "dependabot[bot]" "$BOT_EMAIL" GitHub noreply@github.com "$1"; }

# expect <pass|fail> <label> <base> <head>
expect() {
  local want="$1" label="$2" rc=0
  bash "$check" "$3..$4" >/dev/null 2>&1 || rc=$?
  if { [ "$want" = pass ] && [ "$rc" -eq 0 ]; } || { [ "$want" = fail ] && [ "$rc" -eq 1 ]; }; then
    echo "ok   - $label"
  else
    echo "FAIL - $label (wanted $want, exit $rc)"; fails=$((fails + 1))
  fi
}

human base
base="$(git rev-parse HEAD)"

# Each case builds on top of $base and is reset afterwards.
case_reset() { git reset -q --hard "$base"; }

human "own commit";                     expect pass "human noreply commit passes" "$base" HEAD; case_reset
dependabot "bump dep";                  expect pass "dependabot commit passes" "$base" HEAD; case_reset
commit synthalorian "gmail@example.com" synthalorian "gmail@example.com" "human gmail"
                                        expect fail "human gmail commit fails" "$base" HEAD; case_reset
commit synthalorian "$OK" synthalorian "gmail@example.com" "gmail committer"
                                        expect fail "gmail committer fails" "$base" HEAD; case_reset
dependabot "bump dep"; commit synthalorian "gmail@example.com" synthalorian "gmail@example.com" "gmail"
                                        expect fail "mixed range with one gmail commit fails" "$base" HEAD; case_reset
human "own commit"; dependabot "bump dep"
                                        expect pass "mixed range of own + dependabot passes" "$base" HEAD; case_reset
commit "dependabot[bot]" "gmail@example.com" GitHub noreply@github.com "spoof name"
                                        expect fail "spoofed dependabot[bot] name with gmail email fails" "$base" HEAD; case_reset
commit "dependabot[bot]" "gmail@example.com" "dependabot[bot]" "gmail@example.com" "spoof both"
                                        expect fail "spoofed name on author and committer fails" "$base" HEAD; case_reset
commit "dependabot[bot]" "$BOT_EMAIL" synthalorian "gmail@example.com" "bot author, gmail committer"
                                        expect fail "dependabot author with non-GitHub committer fails" "$base" HEAD; case_reset
commit "dependabot[bot]" "$BOT_EMAIL" synthalorian "$OK" "bot author, own committer"
                                        expect fail "dependabot author with own noreply committer fails" "$base" HEAD; case_reset
commit "dependabot[bot]" "$BOT_EMAIL" GitHub "other@example.com" "bot author, wrong email, GitHub name"
                                        expect fail "dependabot author with GitHub name but other email fails" "$base" HEAD; case_reset
commit synthalorian "$OK" GitHub noreply@github.com "human via web-flow"
                                        expect fail "non-dependabot author with web-flow committer fails" "$base" HEAD; case_reset
commit "some-app[bot]" "1+some-app[bot]@users.noreply.github.com" GitHub noreply@github.com "other bot"
                                        expect fail "other bot is not exempt (no *[bot] wildcard)" "$base" HEAD; case_reset
commit "dependabot[bot]" "49699334+dependabot[bot]@users.noreply.github.com" GitHub noreply@github.com "wrong id"
                                        expect fail "dependabot email with different id fails" "$base" HEAD; case_reset
commit "Dependabot" "$BOT_EMAIL" GitHub noreply@github.com "wrong name"
                                        expect fail "dependabot email with different author name fails" "$base" HEAD; case_reset

# Identities crafted to shift parsed fields must not bypass the check.
commit $'x\x1f'"$OK" "gmail@example.com" "$OK" "" "unit separator in name, empty committer email"
                                        expect fail "separator injection with empty committer email fails" "$base" HEAD; case_reset
commit "x $OK" "gmail@example.com" "$OK" "" "space in name, empty committer email"
                                        expect fail "space in name with empty committer email fails" "$base" HEAD; case_reset
commit synthalorian "$OK" synthalorian "" "empty committer email"
                                        expect fail "empty committer email fails" "$base" HEAD; case_reset
commit synthalorian "" synthalorian "$OK" "empty author email"
                                        expect fail "empty author email fails" "$base" HEAD; case_reset

commit synthalorian "$OK" synthalorian "" "empty committer email, then valid"; human "valid on top"
                                        expect fail "empty committer email below a valid commit fails" "$base" HEAD; case_reset
human "valid"; commit synthalorian "$OK" synthalorian "" "empty committer email on top"
                                        expect fail "empty committer email on top of a valid commit fails" "$base" HEAD; case_reset

# Extra git-log arguments cannot disable or reshape the check.
commit synthalorian "gmail@example.com" synthalorian "gmail@example.com" "gmail with extra args"
rc=0; bash "$check" "$base..HEAD" --oneline >/dev/null 2>&1 || rc=$?
if [ "$rc" -ne 0 ]; then echo "ok   - --oneline cannot bypass the check"; else echo "FAIL - --oneline bypass"; fails=$((fails + 1)); fi
rc=0; bash "$check" "$base..HEAD" --graph >/dev/null 2>&1 || rc=$?
if [ "$rc" -ne 0 ]; then echo "ok   - --graph cannot bypass the check"; else echo "FAIL - --graph bypass"; fails=$((fails + 1)); fi
rc=0; bash "$check" "$base..HEAD" --pretty=oneline >/dev/null 2>&1 || rc=$?
if [ "$rc" -ne 0 ]; then echo "ok   - --pretty cannot bypass the check"; else echo "FAIL - --pretty bypass"; fails=$((fails + 1)); fi
case_reset

# An empty range passes.
expect pass "empty range passes" "$base" "$base"

# Merge commits are still checked.
git checkout -q -b side
commit synthalorian "gmail@example.com" synthalorian "gmail@example.com" "side gmail"
git checkout -q -
human "main work"
GIT_AUTHOR_NAME=synthalorian GIT_AUTHOR_EMAIL="$OK" GIT_COMMITTER_NAME=synthalorian GIT_COMMITTER_EMAIL="$OK" \
  git merge -q --no-ff side -m "merge side" >/dev/null
expect fail "merged-in gmail commit fails" "$base" HEAD
git reset -q --hard "$base"; git branch -q -D side

# A bad revision fails the check (exit 2, not a silent pass).
rc=0; bash "$check" "nonexistent..HEAD" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then echo "ok   - git error fails the check"; else echo "FAIL - git error (exit $rc)"; fails=$((fails + 1)); fi

# The allowed address cannot be overridden from the environment.
rc=0; commit synthalorian "gmail@example.com" synthalorian "gmail@example.com" "env override"
ALLOWED="gmail@example.com" bash "$check" "$base..HEAD" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 1 ]; then echo "ok   - ALLOWED env override is ignored"; else echo "FAIL - env override (exit $rc)"; fails=$((fails + 1)); fi

if [ "$fails" -ne 0 ]; then echo "$fails test(s) failed" >&2; exit 1; fi
echo "all tests passed"
