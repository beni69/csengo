ARG CROSS_BASE_IMAGE
FROM $CROSS_BASE_IMAGE

ARG CROSS_DEB_ARCH
RUN export NODE_MAJOR=16 && \
    apt update && \
    apt install -y ca-certificates curl gnupg apt-transport-https && \
    mkdir -p /etc/apt/keyrings && \
    curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg && \
    echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_$NODE_MAJOR.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list && \
    dpkg --add-architecture ${CROSS_DEB_ARCH} && \
    apt update && \
    apt install -y nodejs libasound2-dev:${CROSS_DEB_ARCH} && \
    npm i -g pnpm
