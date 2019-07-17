FROM fleupold/dfusion-solver-base:v2

RUN apt-get update \ 
    &&  apt-get install -y --no-install-recommends curl ssh git libc-dev gcc openssl pkg-config libssl-dev \
        # listener requirements
        libpq-dev \ 
    && rm -rf /var/lib/apt/lists/*

#Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN ln -s $HOME/.cargo/bin/* /usr/bin/

# Copy driver requirements and install
WORKDIR /app/driver
COPY driver/Cargo.toml ./
RUN mkdir src && touch src/lib.rs && cargo build

# Copy listener requirements and install
WORKDIR /app/listener
RUN USER=docker cargo init
COPY listener/Cargo.toml ./
RUN cargo build