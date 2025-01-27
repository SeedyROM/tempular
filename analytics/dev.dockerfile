FROM rust:1.75-slim

# Install required dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    ca-certificates \
    curl \
    xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Install watchexec using pre-built binary
RUN curl -LO https://github.com/watchexec/watchexec/releases/download/v1.25.1/watchexec-1.25.1-x86_64-unknown-linux-gnu.tar.xz \
    && tar -xf watchexec-1.25.1-x86_64-unknown-linux-gnu.tar.xz \
    && mv watchexec-1.25.1-x86_64-unknown-linux-gnu/watchexec /usr/local/bin/ \
    && rm -rf watchexec-1.25.1-x86_64-unknown-linux-gnu*

WORKDIR /usr/src/app/analytics

# Copy your entire project
COPY . .

# Build with your code
RUN cargo build

# Mount src for hot reloading
VOLUME /usr/src/app/analytics/src

# Use watchexec with clear output options
CMD ["watchexec", "-r", "-w", "src", "--exts", "rs", "--no-vcs-ignore", "--no-project-ignore", "--", "cargo", "run"]
