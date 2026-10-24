#!/usr/bin/env bash
# Reject commits whose author or committer email is not the GitHub noreply address.
# Usage: scripts/check-noreply.sh <rev-range>   e.g. origin/main..HEAD
set -euo pipefail
ALLOWED="${NOREPLY_EMAIL:-212206047+synthalorian@users.noreply.github.com}"
[ $# -ge 1 ] || { echo "usage: check-noreply.sh <rev-range> [git-log args]" >&2; exit 2; }
range="$*"
bad=0
while IFS=' ' read -r sha ae ce; do
  [ -z "$sha" ] && continue
  if [ "$ae" != "$ALLOWED" ] || [ "$ce" != "$ALLOWED" ]; then
    echo "::error::commit ${sha:0:12} author=<$ae> committer=<$ce> (expected <$ALLOWED>)" >&2
    bad=1
  fi
done < <(git log --no-merges --format='%H %ae %ce' "$@" --)
if [ "$bad" -ne 0 ]; then
  echo "Fix with: git rebase -i --exec 'git commit --amend --no-edit --reset-author' <base>" >&2
  exit 1
fi
echo "noreply check passed for $range"
