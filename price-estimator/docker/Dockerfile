FROM rust:1.43-stretch as builder
WORKDIR /usr/src/app
COPY Cargo.toml .
COPY Cargo.lock .
COPY core core
COPY driver driver
COPY dex-contracts dex-contracts
COPY e2e e2e
COPY pricegraph pricegraph
COPY price-estimator price-estimator
RUN ls -l && cargo build --release -p price-estimator

FROM debian:stretch-slim
# Needed so that price-estimator works with https node urls.
RUN apt-get update && apt-get --yes install openssl ca-certificates
COPY --from=builder /usr/src/app/target/release/price-estimator /bin/
ADD docker/rust/tini /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]
CMD ["price-estimator"]

