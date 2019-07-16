FROM gnosispm/dfusion-rust-base:v3

# Copy requirements and install
WORKDIR /app/listener
COPY listener/Cargo.toml ./
RUN cargo build

# Copy smart contract build artefacts
COPY dex-contracts/build dex-contracts/build

# Copy source code
WORKDIR /app/listener
COPY listener/src ./src
RUN cargo clean -p listener && cargo build