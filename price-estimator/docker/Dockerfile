FROM clux/muslrust:1.43.1-stable as builder
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

FROM alpine:latest
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/price-estimator /bin/
RUN apk add -u tini
ENTRYPOINT ["tini", "--"]
CMD ["price-estimator"]
