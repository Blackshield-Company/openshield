#!/usr/bin/env bash
# Reject commits whose author or committer email is not the GitHub noreply address.
# Usage: scripts/check-noreply.sh <rev-range> [extra git-log args]
#   e.g. scripts/check-noreply.sh origin/main..HEAD
#
# Exemption: commits created by Dependabot are accepted. A commit qualifies only
# when ALL of the following match exactly (no wildcards, no other bots):
#   author    name  = dependabot[bot]
#   author    email = 49699333+dependabot[bot]@users.noreply.github.com
#   committer name  = GitHub
#   committer email = noreply@github.com   (GitHub's web-flow committer)
# Any other combination, including a Dependabot author with a different
# committer, is checked against the allowed address as usual.
set -euo pipefail
readonly ALLOWED="212206047+synthalorian@users.noreply.github.com"
readonly BOT_NAME="dependabot[bot]"
readonly BOT_EMAIL="49699333+dependabot[bot]@users.noreply.github.com"
readonly WEBFLOW_NAME="GitHub"
readonly WEBFLOW_EMAIL="noreply@github.com"
[ $# -ge 1 ] || { echo "usage: check-noreply.sh <rev-range> [git-log args]" >&2; exit 2; }
# Capture first so a git failure (bad/missing revision) fails the check.
# One field per line: git strips newlines from identities, so a name or email
# can never shift fields (and empty fields stay empty). Each record ends with a
# "." line so $(...) cannot strip a trailing empty field off the last commit.
# --format comes after "$@" so extra arguments cannot replace it.
if ! log="$(git log "$@" --format='%H%n%an%n%ae%n%cn%n%ce%n.' --)"; then
  echo "::error::git log failed for: $*" >&2
  exit 2
fi
bad=0
[ -n "$log" ] || { echo "noreply check passed for $* (no commits)"; exit 0; }
seen=0
while IFS= read -r sha && IFS= read -r an && IFS= read -r ae && IFS= read -r cn && IFS= read -r ce && IFS= read -r end; do
  if [ "$end" != "." ]; then
    echo "::error::unexpected git log output for: $*" >&2
    exit 2
  fi
  seen=$((seen + 6))
  if [ "$an" = "$BOT_NAME" ] && [ "$ae" = "$BOT_EMAIL" ] \
     && [ "$cn" = "$WEBFLOW_NAME" ] && [ "$ce" = "$WEBFLOW_EMAIL" ]; then
    continue  # Dependabot commit created through GitHub
  fi
  if [ "$ae" != "$ALLOWED" ] || [ "$ce" != "$ALLOWED" ]; then
    echo "::error::commit ${sha:0:12} author=<$ae> committer=<$ce> (expected <$ALLOWED>)" >&2
    bad=1
  fi
done <<< "$log"
# Fail closed if any output was not consumed as a complete record.
total="$(printf '%s\n' "$log" | wc -l | tr -d ' ')"
if [ "$seen" -ne "$total" ]; then
  echo "::error::unexpected git log output for: $*" >&2
  exit 2
fi
if [ "$bad" -ne 0 ]; then
  echo "Fix with: git rebase -i --exec 'git commit --amend --no-edit --reset-author' <base>" >&2
  exit 1
fi
echo "noreply check passed for $*"
