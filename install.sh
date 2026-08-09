#!/bin/sh
# Install prebuilt openspectation binaries from the GitHub releases.
#
# Lives at the repository root rather than under scripts/ because the whole
# point is the shortest URL that can be piped to a shell, and it is POSIX sh
# rather than bash for the same reason: the interpreter is whatever the reader
# typed after the pipe.
#
#   curl -fsSL https://raw.githubusercontent.com/jaimesares/openspectation/main/install.sh | sh
#
# Environment:
#   OPENSPECTATION_VERSION  version to install, with or without the leading v
#                           (default: the latest release)
#   OPENSPECTATION_BIN_DIR  where the binaries go (default: ~/.local/bin)
#   OPENSPECTATION_TARGET   force a target triple, skipping detection

set -eu

REPO="jaimesares/openspectation"
BINARIES="openspectation ospc"
BIN_DIR="${OPENSPECTATION_BIN_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required but was not found"
}

# curl and wget are both common enough that requiring one specifically would
# turn a working machine into a failing one for no reason.
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' --tlsv1.2 "$1"
    else
        wget -qO- "$1"
    fi
}

fetch_to() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    else
        wget -qO "$2" "$1"
    fi
}

command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 ||
    die "either curl or wget is required"
need uname
need tar

# The release archives are built for these eight targets; the four Windows and
# musl variants that are not reachable from here are simply never selected.
detect_target() {
    if [ -n "${OPENSPECTATION_TARGET:-}" ]; then
        printf '%s' "$OPENSPECTATION_TARGET"
        return
    fi

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture: $arch (set OPENSPECTATION_TARGET to override)" ;;
    esac

    case "$os" in
    Linux)
        # A glibc build refuses to start on a musl system, so the wrong guess
        # here is a binary that installs cleanly and then cannot run at all.
        # ldd prints its identity to either stream depending on the libc.
        if [ -n "$(find /lib /lib64 -maxdepth 1 -name 'ld-musl-*' 2>/dev/null)" ] ||
            (ldd --version 2>&1 || true) | grep -qi musl; then
            printf '%s-unknown-linux-musl' "$arch"
        else
            printf '%s-unknown-linux-gnu' "$arch"
        fi
        ;;
    Darwin) printf '%s-apple-darwin' "$arch" ;;
    MINGW* | MSYS* | CYGWIN*)
        die "Windows is not covered by this script; take the .zip from https://github.com/$REPO/releases"
        ;;
    *) die "unsupported system: $os (set OPENSPECTATION_TARGET to override)" ;;
    esac
}

resolve_version() {
    if [ -n "${OPENSPECTATION_VERSION:-}" ]; then
        printf '%s' "${OPENSPECTATION_VERSION#v}"
        return
    fi

    # The tag comes off the API rather than out of the redirect that
    # /releases/latest serves, because reading that redirect means parsing
    # whatever the client prints: curl and the two incompatible wget lineages
    # each report it differently, and one code path cannot be wrong in three
    # ways. Picking the field out with sed keeps a JSON parser off the list of
    # things a machine has to have installed.
    tag="$(fetch "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null |
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
        head -1)"

    case "$tag" in
    v[0-9]*) printf '%s' "${tag#v}" ;;
    *) die "could not determine the latest version; set OPENSPECTATION_VERSION" ;;
    esac
}

# Integrity is checked when the release carries a checksum file and skipped
# loudly when it does not, so publishing checksums later needs no change here.
verify() {
    archive="$1"
    url="$2"

    if command -v sha256sum >/dev/null 2>&1; then
        sum="$(sha256sum "$archive" | cut -d' ' -f1)"
    elif command -v shasum >/dev/null 2>&1; then
        sum="$(shasum -a 256 "$archive" | cut -d' ' -f1)"
    else
        warn "no sha256 tool found; skipping the integrity check"
        return
    fi

    if ! expected="$(fetch "$url.sha256" 2>/dev/null)"; then
        warn "this release publishes no checksum; skipping the integrity check"
        return
    fi

    expected="${expected%% *}"
    [ "$sum" = "$expected" ] ||
        die "checksum mismatch: expected $expected, got $sum"
    say "  checksum ok"
}

target="$(detect_target)"
version="$(resolve_version)"
stem="openspectation-$version-$target"
url="https://github.com/$REPO/releases/download/v$version/$stem.tar.gz"

say "openspectation $version ($target)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

say "  downloading $stem.tar.gz"
fetch_to "$url" "$work/archive.tar.gz" ||
    die "no archive at $url — check that $version was released for $target"

verify "$work/archive.tar.gz" "$url"

tar xzf "$work/archive.tar.gz" -C "$work"

mkdir -p "$BIN_DIR"
for binary in $BINARIES; do
    source_path="$work/$stem/$binary"
    [ -f "$source_path" ] || die "the archive carries no $binary"
    # Replacing a running binary in place fails on some systems; the rename is
    # atomic and leaves nothing half written if the copy is interrupted.
    cp "$source_path" "$BIN_DIR/$binary.new"
    chmod 755 "$BIN_DIR/$binary.new"
    mv -f "$BIN_DIR/$binary.new" "$BIN_DIR/$binary"
    say "  installed $BIN_DIR/$binary"
done

case ":$PATH:" in
*":$BIN_DIR:"*) ;;
*) warn "$BIN_DIR is not in PATH; add it to run openspectation by name" ;;
esac

say "Done. Try: ospc --help"
