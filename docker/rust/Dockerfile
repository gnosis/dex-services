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

ONBUILD COPY dfusion_rust_core ./dfusion_rust_core
ONBUILD COPY driver ./driver
ONBUILD COPY listener ./listener

ONBUILD COPY Cargo.* ./
ONBUILD RUN cargo build

# Build Option 1: Copy already compiled binary from host (used e.g. in travis)
FROM ${SOLVER_BASE} as rust-binary

# Required for listener
ONBUILD RUN apt-get update \
  && apt-get install -y --no-install-recommends libpq-dev libssl1.0.0 libssl-dev ca-certificates \ 
  && rm -rf /var/lib/apt/lists/*

ONBUILD COPY target/debug/stablex stablex
ONBUILD COPY target/debug/listener graph-listener
ONBUILD COPY target/debug/dfusion dfusion

# Trigger actual build
FROM ${RUST_BASE}