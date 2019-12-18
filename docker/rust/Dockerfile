FROM gnosispm/dfusion-rust-base:v4

# Install solver if specified
ARG use_solver
WORKDIR /app
COPY docker/rust/clone_solver.sh docker/rust/clone_solver.sh
COPY .ssh/ .ssh/
RUN if [ "$use_solver" = 1 ]; then docker/rust/clone_solver.sh; else echo "Not installing solver"; fi && rm -rf .ssh
ENV LINEAR_OPTIMIZATION_SOLVER $use_solver
ENV PYTHONPATH ${PYTHONPATH}:/app/batchauctions/

# Copy and build code
COPY dex-contracts/build ./dex-contracts/build
COPY dfusion_rust_core ./dfusion_rust_core
COPY driver ./driver
COPY listener ./listener

COPY Cargo.* ./
RUN cargo build
