#!/bin/sh
# Sets the version, commits it on main, tags vVERSION and pushes both. The
# tag starts .github/workflows/release.yml, which builds the DMG and
# publishes the GitHub release.
#
#   scripts/release.sh 0.2.0
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 VERSION" >&2
  exit 2
fi

version=$1
tag="v$version"
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "version must look like 1.2.3, got $version" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"

if [ "$(git branch --show-current)" != "main" ]; then
  echo "release from main" >&2
  exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "the working tree has uncommitted changes" >&2
  exit 1
fi
git fetch --quiet --tags origin main
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
  echo "main is not in sync with origin/main" >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag already exists" >&2
  exit 1
fi

current=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ "$current" != "$version" ]; then
  # Only the first `version =` line is the package's own.
  sed -i '' "1,/^version = /s/^version = \".*\"$/version = \"$version\"/" Cargo.toml
  cargo metadata --format-version 1 >/dev/null
  git add Cargo.toml Cargo.lock
  git commit --quiet -m "Release $tag"
fi

git tag -a "$tag" -m "ss $version"
git push --atomic origin main "$tag"
echo "Pushed $tag. The release workflow builds the DMG and publishes it:"
echo "  https://github.com/patryksztuczka/ss/actions/workflows/release.yml"
