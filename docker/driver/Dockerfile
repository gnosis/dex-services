FROM rust:1.31

RUN rustup self update && rustup toolchain uninstall stable && rustup toolchain install stable

WORKDIR /app

COPY . .
