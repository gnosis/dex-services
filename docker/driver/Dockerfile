FROM fleupold/dfusion-solver-base:v1

#Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN ln -s $HOME/.cargo/bin/* /usr/bin/

# Copy requirements and install
WORKDIR /app/driver
COPY driver/Cargo.toml .
RUN mkdir src && touch src/lib.rs
RUN cargo build

# Install solver if specified
ARG use_solver
WORKDIR /app
COPY docker/driver/clone_solver.sh docker/driver/clone_solver.sh
COPY .ssh/ .ssh/
RUN if [ "$use_solver" = 1 ]; then docker/driver/clone_solver.sh; else echo "Not installing solver"; fi
ENV PYTHONPATH ${PYTHONPATH}:/app/batchauctions/

# Copy smart contract build artefacts
COPY dex-contracts/build dex-contracts/build

# Copy source code
WORKDIR /app/driver
COPY driver/src ./
RUN cargo build