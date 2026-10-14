# OpenShield packaging drafts

Drafts only. Nothing in this directory has been published.

- Version: 1.1.1
- License: Apache-2.0
- Repository: https://github.com/Blackshield-Company/openshield
- Maintainer: synthalorian <212206047+synthalorian@users.noreply.github.com>
- Release asset: `openshield-<target>.tar.gz` extracts to `openshield-<target>/` containing `openshield`, `LICENSE`, `README.md`, and `CHANGELOG.md`
- Tag archive: https://github.com/Blackshield-Company/openshield/archive/refs/tags/v1.1.1.tar.gz

## What each draft is

| Path | What it is |
| --- | --- |
| `aur/openshield-bin/` | AUR binary package. Installs from `$srcdir/openshield-$CARCH-unknown-linux-gnu/` (`CARCH` `x86_64`/`aarch64` maps to that Rust triple) to `/usr/bin/openshield`, plus `LICENSE` and `README.md` from the tarball. `provides`/`conflicts` `openshield`. `depends=(gcc-libs glibc)`. Per-arch `source_*` and `sha256sums_*` only. |
| `aur/openshield/` | AUR source package. Builds the v1.1.1 tag archive with cargo. No extra `--features` (gateways are default in 1.1.1). Installs the binary, `LICENSE`, and `README.md`. Conflicts with `openshield-bin`. `options=(!lto)` because the crate already sets release LTO and makepkg LTO breaks the bundled sqlite build. |
| `homebrew/openshield.rb` | Formula for the `Blackshield-Company/homebrew-tap` tap (`brew tap Blackshield-Company/tap`). Prebuilt release tarballs for macOS arm/intel and Linux arm/intel. `bin.install "openshield"` is enough: brew strips the single top-level dir. `prefix.install` also ships `LICENSE` and `README.md`. |

The tag archive unpacks to `openshield-1.1.1` (GitHub strips a leading `v`). The source `PKGBUILD` `cd`s there. The release tarball is different: it unpacks to `openshield-<target>/`.

## crates.io exclude

`Cargo.toml` `exclude` drops `*.whl`, `docs/`, `packaging/`, and `.github/` from the published crate. Kept: `src/`, `build.rs`, `Cargo.toml`, `Cargo.lock`, `README.md`, `LICENSE`, `CHANGELOG.md`.

Checked against the build:

- `build.rs` only calls `git` and stamps `OS_GIT_HASH` / `OS_BUILD_DATE`. It does not read `docs/`, `packaging/`, `.github/`, wheels, or pngs. Outside a git checkout it falls back to `unknown`.
- `README.md` references root `openshield.png` (the only root png). That file is not excluded.
- `assets/sprites/` is **not** excluded. `blackshield-mercenary.ans` is 7.7K, not large, and `src/tui/sprites.rs` embeds it with `include_str!`. Excluding it would break the build.
- `assets/openshield_shield.png` (225K) is unused by the build and README. Left in; drop it later if the crate should be smaller.

## Validate

Do this before any publish. These commands are not part of creating the drafts.

AUR (each package directory):

```bash
namcap PKGBUILD
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD .SRCINFO
```

`makepkg --printsrcinfo` is the source of truth for `.SRCINFO`. The copies here match the `PKGBUILD`s by hand; regenerate them so field order matches the local makepkg.

A full build (downloads sources, runs cargo) is `makepkg -f` in `aur/openshield` or `makepkg -f` in `aur/openshield-bin`. Not required to review the drafts.

Homebrew, from a checkout of this repo or after copying the formula into the tap:

```bash
brew audit --new --formula packaging/homebrew/openshield.rb
```

`brew style` may also want the frozen-string comment (already present). A custom tap may still warn that the formula ships a prebuilt binary; that is intentional and is not a homebrew-core submission.

crates.io:

```bash
cargo publish --dry-run
```

Confirm the dry-run file list includes `assets/sprites/blackshield-mercenary.ans`, `openshield.png`, `README.md`, `LICENSE`, `CHANGELOG.md`, `build.rs`, and `src/`, and does not include `*.whl`, `docs/`, `packaging/`, or `.github/`.

`namcap` on a built package may report extra shared libraries. Update `depends=` from that output; do not guess.

## Bump

1. Set `pkgver` in both `PKGBUILD`s, `version` in `openshield.rb`, and the expanded versions in both `.SRCINFO` files.
2. Replace every release-tarball sha256 and the tag-archive sha256. Homebrew URLs are pinned to `v1.1.1`; update those too (the `PKGBUILD` release URLs follow `pkgver`).
3. In each AUR directory, run `updpkgsums` to refresh sums. Then `makepkg --printsrcinfo > .SRCINFO`.
4. Re-run the validate commands above.

## Publishing requires synth's approval: AUR account push, tap repo creation, crates.io publish

Do not push to the AUR, create `Blackshield-Company/homebrew-tap`, or run `cargo publish` without synth's approval.

The crates.io name `openshield` was unclaimed as of 2026-10-14. Re-check `cargo search openshield` (or the crates.io page) immediately before any publish; the name can be taken in the meantime.
