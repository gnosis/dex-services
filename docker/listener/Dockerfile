FROM fleupold/dfusion-rust-base:v1

# Copy requirements and install
WORKDIR /app/listener
COPY listener/Cargo.* ./
COPY dfusion_rust_core /app/dfusion_rust_core
RUN cargo build

# Copy smart contract build artifacts
COPY dex-contracts/build dex-contracts/build

# Copy source code
WORKDIR /app/listener
COPY listener/src ./src
RUN cargo clean -p listener && cargo build