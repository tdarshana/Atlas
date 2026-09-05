#!/usr/bin/env bash
# Publish the code to GitHub without the working notes.
#
# The local history carries docs/, references/, .superpowers/ and .vscode/, which are
# not published. Rather than rewriting the local repository, this clones it into a
# scratch directory, strips those paths from every commit with git filter-repo, merges
# the remote's existing history underneath (so a plain push is a fast-forward and the
# remote's own commits are kept), and pushes the result as `main`.
#
#   scripts/publish.sh            # push local main
#   scripts/publish.sh --dry-run  # do everything but the push
#
# Needs git-filter-repo (brew install git-filter-repo). The clone is deleted afterwards.
set -euo pipefail

remote_url="${ATLAS_PUBLISH_URL:-https://github.com/tdarshana/Atlas.git}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dry_run=0
[ "${1:-}" = "--dry-run" ] && dry_run=1

if ! git -C "$root" diff --quiet || ! git -C "$root" diff --cached --quiet; then
  echo "publish: commit or stash your changes first" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git clone -q --no-local --branch main "$root" "$work/atlas"
cd "$work/atlas"

# Strip the working notes from every commit, and publish under the GitHub handle and
# its noreply address rather than a personal name or mailbox (GitHub still attributes
# the commits to the account through the noreply address).
handle="${ATLAS_PUBLISH_HANDLE:-tdarshana}"
git filter-repo --quiet --force --invert-paths \
  --path docs --path references --path .superpowers --path .vscode \
  --name-callback "return b'$handle'" \
  --email-callback "return b'$handle@users.noreply.github.com'"

git remote add origin "$remote_url"
git fetch -q origin main || true
if git rev-parse -q --verify origin/main >/dev/null; then
  # Keep the remote's commits as ancestors; ours win on any path both sides touched.
  git merge -q --allow-unrelated-histories -X ours -m "Publish $(git -C "$root" rev-parse --short main)" origin/main
fi

echo "publish: $(git rev-list --count HEAD) commits, $(git ls-files | wc -l | tr -d ' ') files"
git ls-files | cut -d/ -f1 | sort -u | tr '\n' ' '; echo
if [ "$dry_run" = 1 ]; then
  echo "publish: dry run, not pushing"
  exit 0
fi
git push -q origin HEAD:main
echo "publish: pushed to $remote_url"
