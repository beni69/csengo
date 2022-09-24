ARG CROSS_BASE_IMAGE
FROM $CROSS_BASE_IMAGE

ARG CROSS_DEB_ARCH
RUN curl -fsSL https://deb.nodesource.com/setup_16.x | bash - && \
    dpkg --add-architecture ${CROSS_DEB_ARCH} && \
    apt update && \
    apt install -y nodejs libasound2-dev:${CROSS_DEB_ARCH} && \
    npm i -g pnpm
