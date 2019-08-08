FROM fleupold/dfusion-rust-base:v2

# Install solver if specified
ARG use_solver
WORKDIR /app
COPY docker/rust/clone_solver.sh docker/rust/clone_solver.sh
COPY .ssh/ .ssh/
RUN if [ "$use_solver" = 1 ]; then docker/rust/clone_solver.sh; else echo "Not installing solver"; fi
ENV LINEAR_OPTIMIZATION_SOLVER $use_solver
ENV PYTHONPATH ${PYTHONPATH}:/app/batchauctions/

# Copy smart contract build artifacts
COPY dex-contracts/build dex-contracts/build

# Copy and build code
COPY graph_node_reader ./graph_node_reader
COPY dfusion_rust_core ./dfusion_rust_core
COPY driver ./driver
COPY listener ./listener

COPY Cargo.* ./
RUN cargo build