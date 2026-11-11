FROM ghcr.io/actions/actions-runner:2.337.0@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4
USER root
RUN apt-get update && apt-get install -y --no-install-recommends build-essential libdbus-1-dev pkg-config libssl-dev curl ca-certificates xz-utils libatomic1 time \
    && rm -rf /var/lib/apt/lists/*
COPY job-started.sh /opt/runner-job-started.sh
RUN chmod 755 /opt/runner-job-started.sh
USER runner
ENV PATH="/home/runner/.cargo/bin:/home/runner/externals/node24/bin:${PATH}"
ENV CARGO_BUILD_JOBS=2
# Release-specific installer bytes, verified before execution. Do not replace
# these digests with checksums downloaded in the same build as the installer.
# Source: https://static.rust-lang.org/rustup/archive/1.28.2/<target>/rustup-init.sha256
RUN set -eu; \
    case "$(uname -m)" in \
      x86_64) target=x86_64-unknown-linux-gnu; sha256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c ;; \
      aarch64) target=aarch64-unknown-linux-gnu; sha256=e3853c5a252fca15252d07cb23a1bdd9377a8c6f3efa01531109281ae47f841c ;; \
      *) echo "unsupported Rust installer architecture" >&2; exit 1 ;; \
    esac; \
    curl --proto '=https' --tlsv1.2 -fLsS "https://static.rust-lang.org/rustup/archive/1.28.2/$target/rustup-init" -o /tmp/rustup-init; \
    printf '%s  /tmp/rustup-init\n' "$sha256" | sha256sum -c -; \
    chmod 700 /tmp/rustup-init; \
    /tmp/rustup-init -y --profile minimal --default-toolchain 1.99.1 --component rustfmt,clippy; \
    rm /tmp/rustup-init
WORKDIR /home/runner
USER root
RUN apt-get update && apt-get install -y --no-install-recommends fonts-dejavu-core fonts-liberation \
    && rm -rf /var/lib/apt/lists/*
USER runner
