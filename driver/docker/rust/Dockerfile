# With this image, only the naive solver will work
ARG SOLVER_BASE=ubuntu:bionic

FROM ${SOLVER_BASE}

ARG BINARY_PATH=target/debug/driver

# Required for listener
RUN apt-get update \
  && apt-get install -y --no-install-recommends libpq-dev libssl1.0.0 libssl-dev ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY ${BINARY_PATH} /stablex
CMD ["/stablex"]

# Add Tini
# We ran into github rate limiting using this url so we now keep a local copy of
# tini instead.
# ENV TINI_VERSION v0.18.0
# ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini /tini
ADD driver/docker/rust/tini /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]
