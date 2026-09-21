FROM ghcr.io/actions/actions-runner:2.337.0@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4
USER root
RUN apt-get update && apt-get install -y --no-install-recommends build-essential libdbus-1-dev pkg-config libssl-dev curl ca-certificates xz-utils libatomic1 time \
    && rm -rf /var/lib/apt/lists/*
COPY job-started.sh /opt/runner-job-started.sh
RUN chmod 755 /opt/runner-job-started.sh
USER runner
ENV PATH="/home/runner/.cargo/bin:/home/runner/externals/node24/bin:${PATH}"
ENV CARGO_BUILD_JOBS=2
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --component rustfmt,clippy
WORKDIR /home/runner
USER root
RUN apt-get update && apt-get install -y --no-install-recommends fonts-dejavu-core fonts-liberation \
    && rm -rf /var/lib/apt/lists/*
USER runner
