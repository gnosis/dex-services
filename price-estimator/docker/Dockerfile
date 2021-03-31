FROM clux/muslrust:1.49.0 as builder
WORKDIR /usr/src/app

COPY . .

RUN ls -l && cargo build --release -p price-estimator

FROM alpine:latest
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/price-estimator /bin/
RUN apk add -u tini
ENTRYPOINT ["tini", "--"]
CMD ["price-estimator"]
