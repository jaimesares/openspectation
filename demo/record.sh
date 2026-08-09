#!/usr/bin/env bash
# Records every demo tape, or the ones named on the command line.
#
#   ./record.sh                 # all of them
#   ./record.sh 03-tasks        # one, by stem
#   ./record.sh 00-calibrate    # the grid check, before touching the settings
#
# The fixture workspace is reseeded before each tape: recordings that toggle a
# task or save an edit would otherwise leak into the next one.

set -euo pipefail

DEMO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(dirname "$DEMO")"
CARGO="${CARGO:-${HOME}/.cargo/bin/cargo}"

cd "$DEMO"

command -v vhs >/dev/null 2>&1 || {
    echo "error: vhs not found — sudo dnf5 install vhs" >&2
    exit 1
}

echo "==> building"
"$CARGO" build --release --manifest-path "$REPO/Cargo.toml" --quiet

# `Require ospc` and the tapes themselves resolve the binary from PATH.
export PATH="$REPO/target/release:$PATH"

mkdir -p out

# A tape that opens the preferences panel writes a settings file. Point it at a
# scratch path so recording never edits the preferences of whoever is recording.
export OPENSPECTATION_SETTINGS="$DEMO/out/.settings.yaml"

if [ "$#" -gt 0 ]; then
    tapes=()
    for stem in "$@"; do
        tapes+=("tapes/${stem%.tape}.tape")
    done
else
    tapes=(tapes/*.tape)
fi

for tape in "${tapes[@]}"; do
    [ -f "$tape" ] || {
        echo "error: no such tape: $tape" >&2
        exit 1
    }
    echo "==> $tape"
    ./seed.sh >/dev/null
    rm -f "$OPENSPECTATION_SETTINGS"
    vhs "$tape"
done

./seed.sh >/dev/null
rm -f "$OPENSPECTATION_SETTINGS"

echo
echo "==> output"
ls -1sh out | tail -n +2
