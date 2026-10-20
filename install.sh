#!/usr/bin/env bash
# OpenShield installer
#
#   curl -fsSL https://raw.githubusercontent.com/synthalorian/openshield/main/install.sh | bash
#
# Downloads the prebuilt release binary for your OS/arch from GitHub Releases,
# verifies its SHA-256 checksum, and installs it to $BIN_DIR. If no prebuilt
# binary is available, falls back to `cargo install --git`.
#
# Environment overrides:
#   OPENSHIELD_VERSION  release tag to install (default: latest), e.g. v1.1.0
#   BIN_DIR             install directory (default: ~/.local/bin)
#   OPENSHIELD_FROM_SOURCE=1  skip the binary download, build with cargo
#   DRY_RUN=1           print what would happen without downloading/installing
set -euo pipefail

OWNER_REPO="synthalorian/openshield"
REPO_URL="https://github.com/${OWNER_REPO}"
VERSION="${OPENSHIELD_VERSION:-latest}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
DRY_RUN="${DRY_RUN:-0}"

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; }
run() {
    if [ "$DRY_RUN" = "1" ]; then
        say "[dry-run] $*"
    else
        "$@"
    fi
}

# Map uname output to a Rust target triple (matches release asset names).
detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$arch" in
        x86_64 | amd64) arch="x86_64" ;;
        aarch64 | arm64) arch="aarch64" ;;
        *) err "unsupported architecture: $arch"; return 1 ;;
    esac
    case "$os" in
        Linux) echo "${arch}-unknown-linux-gnu" ;;
        Darwin) echo "${arch}-apple-darwin" ;;
        MINGW* | MSYS* | CYGWIN*)
            if [ "$arch" = "x86_64" ]; then
                echo "x86_64-pc-windows-msvc"
            else
                err "unsupported Windows architecture: $arch"; return 1
            fi
            ;;
        *) err "unsupported OS: $os"; return 1 ;;
    esac
}

download() { # url dest
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif command -v wget > /dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        err "need curl or wget"; return 1
    fi
}

sha256_of() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo ""
    fi
}

release_base_url() {
    if [ "$VERSION" = "latest" ]; then
        echo "${REPO_URL}/releases/latest/download"
    else
        echo "${REPO_URL}/releases/download/${VERSION}"
    fi
}

install_binary() {
    local target="$1" ext archive base tmp exe
    case "$target" in
        *windows*) ext="zip"; exe="openshield.exe" ;;
        *) ext="tar.gz"; exe="openshield" ;;
    esac
    archive="openshield-${target}.${ext}"
    base="$(release_base_url)"
    say "Downloading ${base}/${archive}"
    if [ "$DRY_RUN" = "1" ]; then
        say "[dry-run] verify ${archive}.sha256, extract ${exe} -> ${BIN_DIR}/${exe}"
        return 0
    fi

    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" EXIT
    if ! download "${base}/${archive}" "${tmp}/${archive}"; then
        err "no prebuilt binary at ${base}/${archive}"
        return 1
    fi
    if download "${base}/${archive}.sha256" "${tmp}/${archive}.sha256"; then
        local expected actual
        expected="$(awk '{print $1}' "${tmp}/${archive}.sha256")"
        actual="$(sha256_of "${tmp}/${archive}")"
        if [ -z "$actual" ]; then
            say "warning: no sha256 tool found, skipping checksum verification"
        elif [ "$expected" != "$actual" ]; then
            err "checksum mismatch for ${archive}: expected ${expected}, got ${actual}"
            exit 1
        else
            say "Checksum OK (${actual})"
        fi
    else
        say "warning: checksum file not found, skipping verification"
    fi

    case "$ext" in
        zip) unzip -q "${tmp}/${archive}" -d "${tmp}/x" ;;
        *) mkdir -p "${tmp}/x" && tar -xzf "${tmp}/${archive}" -C "${tmp}/x" ;;
    esac
    local found
    found="$(find "${tmp}/x" -type f -name "$exe" | head -n 1)"
    if [ -z "$found" ]; then
        err "${exe} not found in ${archive}"
        exit 1
    fi
    mkdir -p "$BIN_DIR"
    install -m 0755 "$found" "${BIN_DIR}/${exe}" 2> /dev/null \
        || { cp "$found" "${BIN_DIR}/${exe}" && chmod 0755 "${BIN_DIR}/${exe}"; }
    say "Installed ${BIN_DIR}/${exe}"
}

install_from_source() {
    say "Building from source with cargo (this can take a while)..."
    if ! command -v cargo > /dev/null 2>&1; then
        err "cargo not found. Install Rust from https://rustup.rs and re-run this script."
        exit 1
    fi
    local args=(install --git "${REPO_URL}.git" --locked --root "$(dirname "$BIN_DIR")" openshield)
    if [ "$VERSION" != "latest" ]; then
        args+=(--tag "$VERSION")
    fi
    run cargo "${args[@]}"
    if [ "$(dirname "$BIN_DIR")/bin" != "$BIN_DIR" ] && [ "$DRY_RUN" != "1" ]; then
        mkdir -p "$BIN_DIR"
        mv "$(dirname "$BIN_DIR")/bin/openshield" "$BIN_DIR/"
    fi
}

main() {
    say "OpenShield installer (${OWNER_REPO}, version: ${VERSION})"
    local target=""
    if [ "${OPENSHIELD_FROM_SOURCE:-0}" != "1" ]; then
        target="$(detect_target)" || target=""
    fi
    if [ -n "$target" ]; then
        say "Detected target: ${target}"
        if ! install_binary "$target"; then
            say "Falling back to building from source."
            install_from_source
        fi
    else
        install_from_source
    fi

    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) say "Note: ${BIN_DIR} is not on your PATH. Add it with:"
           say "  export PATH=\"${BIN_DIR}:\$PATH\"" ;;
    esac
    say "Done. Run 'openshield setup' to configure a provider, then 'openshield'."
}

main "$@"
