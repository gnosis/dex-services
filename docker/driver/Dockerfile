FROM fleupold/dfusion-solver-base:v1

#Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

# Copy data
WORKDIR /app
COPY . .

# Install solver if specified
ARG use_solver
RUN if [ "$use_solver" = 1 ]; then docker/driver/clone_solver.sh; else echo "Not installing solver"; fi

# Install Rust dependencies
WORKDIR /app/driver
RUN $HOME/.cargo/bin/cargo build