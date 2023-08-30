#!/bin/sh
set -e

rm -vf csengo-x86_64-unknown-linux-gnu.zip
wget https://nightly.link/beni69/csengo/workflows/ci/main/csengo-x86_64-unknown-linux-gnu.zip

rm -vf csengo
unzip csengo-x86_64-unknown-linux-gnu.zip
chmod +x ./csengo

systemctl --user restart csengo.service
