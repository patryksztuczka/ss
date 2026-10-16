# Releasing

A release is a `vX.Y.Z` tag on `main` with `ss-X.Y.Z-arm64.dmg` attached to its GitHub release.
The version lives in `Cargo.toml` only; the app shows it in Settings and `ss --version`.

## Cut a release

From a clean `main` that matches `origin/main`:

```sh
scripts/release.sh 0.1.0
```

The script sets the version in `Cargo.toml` and `Cargo.lock` (committing "Release vX.Y.Z" if it
changed), creates the annotated tag and pushes `main` and the tag together. The tag starts
[`.github/workflows/release.yml`](../.github/workflows/release.yml), which:

1. fails if the tag doesn't match the `Cargo.toml` version,
2. runs `scripts/bundle.sh` on a `macos-15` (Apple silicon) runner,
3. creates the GitHub release with generated notes and the DMG.

Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and
`cargo test` before releasing; the workflow only builds.

If the workflow fails, fix it on `main`, then delete the tag and run the script again with the
same version:

```sh
git push --delete origin v0.1.0 && git tag -d v0.1.0
scripts/release.sh 0.1.0
```

## Build locally

```sh
scripts/bundle.sh
```

This writes `target/release/ss.app` and `target/release/ss-<version>-arm64.dmg`. The bundle is
`dev.sztuczka.ss`, `LSUIElement`, macOS 15+, arm64, with the icon from `resources/icon.png`.

## Installing an unsigned build

The app is ad-hoc signed, not signed with a Developer ID or notarized. On first launch of a
downloaded copy macOS refuses to open it. Either:

- open System Settings → Privacy & Security and click **Open Anyway** next to the ss message, or
- run `xattr -dr com.apple.quarantine /Applications/ss.app`.

Each new build has a different ad-hoc signature, so macOS may treat an update as a new app for
Screen Recording. If captures fail after updating, remove ss from System Settings → Privacy &
Security → Screen & System Audio Recording and allow it again. Developer ID signing fixes both
and is the next step when it's worth it.
