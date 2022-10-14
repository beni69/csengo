# See here for image contents: https://github.com/microsoft/vscode-dev-containers/tree/v0.245.2/containers/rust/.devcontainer/base.Dockerfile

# [Choice] Debian OS version (use bullseye on local arm64/Apple Silicon): buster, bullseye
ARG VARIANT="bullseye"
FROM mcr.microsoft.com/vscode/devcontainers/rust:0-${VARIANT}

RUN export DEBIAN_FRONTEND=noninteractive && \
    (curl -fsSL https://deb.nodesource.com/setup_16.x | bash -) && \
    apt-get update && \
    apt-get upgrade -y --no-install-recommends && \
    apt-get install -y --no-install-recommends nodejs libasound2-dev && \
    su vscode -c "cargo install cargo-bloat cargo-cache cross" && \
    npm i -g pnpm
