#!/bin/sh
set -e

rm -v csengo-x86_64-unknown-linux-gnu.zip
wget https://nightly.link/beni69/csengo/workflows/ci/main/csengo-x86_64-unknown-linux-gnu.zip

rm -v csengo
unzip csengo-x86_64-unknown-linux-gnu.zip
chmod +x ./csengo

systemctl --user restart csengo.service
