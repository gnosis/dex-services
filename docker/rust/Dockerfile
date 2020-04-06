# With this image, only the naive solver will work
ARG SOLVER_BASE=ubuntu:bionic

ARG RUST_BASE=rust-compile

# Build Option 1: Download rust and compile sources
FROM ${SOLVER_BASE} as rust-compile
ONBUILD RUN apt-get update \
  &&  apt-get install -y --no-install-recommends curl ca-certificates \
  # common crate requirements
  gcc libc-dev libssl-dev pkg-config \
  # listener requirements
  libpq-dev \
  # remove apt package info
  && rm -rf /var/lib/apt/lists/* \
  #Install Rust
  && curl https://sh.rustup.rs -sSf | sh -s -- -y \
  && ln -s $HOME/.cargo/bin/* /usr/bin/

ONBUILD COPY dex-contracts/build ./dex-contracts/build
ONBUILD COPY driver ./driver
ONBUILD COPY e2e ./e2e

ONBUILD COPY Cargo.* ./
ONBUILD RUN cargo build

# Build Option 2: Copy already compiled binary from host (used e.g. in travis)
FROM ${SOLVER_BASE} as rust-binary

# Required for listener
ONBUILD RUN apt-get update \
  && apt-get install -y --no-install-recommends libpq-dev libssl1.0.0 libssl-dev ca-certificates \
  && rm -rf /var/lib/apt/lists/*

ONBUILD COPY target/debug/driver stablex

# Trigger actual build
FROM ${RUST_BASE}

# Add Tini
# We ran into github rate limiting using this url so we now keep a local copy of
# tini instead.
# ENV TINI_VERSION v0.18.0
# ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini /tini
ADD docker/rust/tini /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]
