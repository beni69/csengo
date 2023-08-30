#!/bin/sh
# https://stackoverflow.com/a/20816534
SCRIPTNAME="${0##*/}"
warn() {
    printf >&2 "$SCRIPTNAME: $*\n"
}
iscmd() {
    command -v >&- "$@"
}
checkdeps() {
    local -i not_found
    for cmd; do
        iscmd "$cmd" || {
            warn $"$cmd is not found"
            let not_found++
        }
    done
    (( not_found == 0 )) || {
        warn $"Install dependencies listed above to use $SCRIPTNAME"
        exit 1
    }
}

checkdeps wget unzip chmod systemctl

chmod +x misc/update.sh

wget https://nightly.link/beni69/csengo/workflows/ci/main/csengo-x86_64-unknown-linux-gnu.zip
unzip csengo-x86_64-unknown-linux-gnu.zip
chmod +x ./csengo
touch env

cd misc
mkdir -vp ~/.config/systemd/user/
ln -sv $PWD/csengo.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now csengo.service
systemctl --user status csengo.service
