FROM gnosispm/dfusion-rust-base:v3

# Copy requirements and install
WORKDIR /app/driver
COPY driver/Cargo.toml ./
RUN cargo build

# Install solver if specified
ARG use_solver
WORKDIR /app
COPY docker/driver/clone_solver.sh docker/driver/clone_solver.sh
COPY .ssh/ .ssh/
RUN if [ "$use_solver" = 1 ]; then docker/driver/clone_solver.sh; else echo "Not installing solver"; fi
ENV LINEAR_OPTIMIZATION_SOLVER $use_solver
ENV PYTHONPATH ${PYTHONPATH}:/app/batchauctions/

# Copy smart contract build artefacts
COPY dex-contracts/build dex-contracts/build

# Copy source code
WORKDIR /app/driver
COPY driver/src ./src
RUN cargo clean -p driver && cargo build