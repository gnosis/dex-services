FROM gnosispm/dfusion-solver-base:v2

RUN apt-get update \ 
  &&  apt-get install -y --no-install-recommends curl ssh git libc-dev gcc openssl pkg-config libssl-dev \
  # listener requirements
  libpq-dev \ 
  && rm -rf /var/lib/apt/lists/*

#Install Rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN ln -s $HOME/.cargo/bin/* /usr/bin/

WORKDIR /app/
COPY dfusion_rust_core ./dfusion_rust_core
COPY driver ./driver
COPY listener ./listener

COPY Cargo.* ./
RUN cargo build
