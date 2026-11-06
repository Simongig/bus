#!/usr/bin/env bash
# Bump the version, commit, tag v<version> and push. The tag triggers the release workflow.
#
#   scripts/release.sh [patch|minor|major|X.Y.Z]
#
# With no argument the current Cargo.toml version is released as-is (e.g. the first release).
# Set YES=1 to skip the confirmation prompt.
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "error: $*" >&2; exit 1; }

bump="${1:-}"
current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
[[ $current =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || die "can't parse Cargo.toml version '$current'"
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

case "$bump" in
  "")    new=$current ;;
  patch) new="$major.$minor.$((patch + 1))" ;;
  minor) new="$major.$((minor + 1)).0" ;;
  major) new="$((major + 1)).0.0" ;;
  *)     [[ $bump =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "BUMP must be patch, minor, major or X.Y.Z (got '$bump')"
         new=$bump ;;
esac
tag="v$new"

# Preconditions: nothing is changed until all of these pass.
[[ -z $(git status --porcelain) ]] || die "working tree not clean"
[[ $(git rev-parse --abbrev-ref HEAD) == main ]] || die "not on main"
git fetch -q origin main
[[ $(git rev-parse HEAD) == $(git rev-parse origin/main) ]] || die "main is not in sync with origin/main (push or pull first)"
! git rev-parse -q --verify "refs/tags/$tag" >/dev/null || die "tag $tag already exists"
if [[ $new != "$current" ]]; then
  [[ $(printf '%s\n%s\n' "$current" "$new" | sort -V | tail -n1) == "$new" ]] || die "$new is not newer than $current"
fi

make ci

if [[ $new == "$current" ]]; then
  echo "Release $tag (current version, no bump)"
else
  echo "Release $current -> $new ($tag)"
fi
if [[ ${YES:-} != 1 ]]; then
  read -r -p "Commit, tag and push to origin? [y/N] " answer
  [[ $answer == [yY]* ]] || die "aborted, nothing changed"
fi

if [[ $new != "$current" ]]; then
  sed -i "0,/^version = \".*\"/s//version = \"$new\"/" Cargo.toml
  cargo check --quiet          # updates the version in Cargo.lock without touching dependencies
  git add Cargo.toml Cargo.lock
  git commit -q -m "Release $tag"
fi
git tag -a "$tag" -m "bus $tag"
git push --atomic origin main "$tag"
echo "Pushed $tag; the release workflow will publish it."
