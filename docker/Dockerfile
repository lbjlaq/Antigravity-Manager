# --- Frontend Build Stage ---
FROM node:20-slim AS frontend-builder
ARG USE_MIRROR=auto
WORKDIR /app

# Use npm mirror if needed
RUN if [ "$USE_MIRROR" = "true" ] || ( [ "$USE_MIRROR" = "auto" ] && ! timeout 3 bash -c "</dev/tcp/www.google.com/80" 2>/dev/null ); then \
        echo "Using npm mirror..."; \
        npm config set registry https://registry.npmmirror.com; \
    else \
        echo "Using default npm registry..."; \
    fi

COPY package*.json ./
RUN npm install
COPY . .
RUN npm run build

# --- Backend Build Stage ---
FROM rust:1.85-slim AS backend-builder
ARG USE_MIRROR=auto

# Conditionally use Aliyun mirror for APT
RUN if [ "$USE_MIRROR" = "true" ] || ( [ "$USE_MIRROR" = "auto" ] && ! timeout 3 bash -c "</dev/tcp/www.google.com/80" 2>/dev/null ); then \
        echo "Using Aliyun mirror for APT..."; \
        sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list.d/debian.sources || \
        sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list; \
    else \
        echo "Using default APT sources..."; \
    fi

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*

# Use Aliyun mirror for Cargo if needed (Sparse Index)
ENV CARGO_HTTP_MULTIPLEXING=false
RUN if [ "$USE_MIRROR" = "true" ] || ( [ "$USE_MIRROR" = "auto" ] && ! timeout 3 bash -c "</dev/tcp/www.google.com/80" 2>/dev/null ); then \
        echo "Using Aliyun mirror for Cargo..."; \
        mkdir -p /root/.cargo && \
        echo "[source.crates-io]\nreplace-with = 'aliyun'\n\n[source.aliyun]\nregistry = \"sparse+https://mirrors.aliyun.com/crates.io-index/\"" > /root/.cargo/config.toml; \
    else \
        echo "Using default Cargo registry..."; \
    fi

WORKDIR /app
# Copy the whole project to build the Rust workspace
COPY . .

# Build the Rust backend in release mode
WORKDIR /app/src-tauri
RUN cargo build --release --bin antigravity_tools

# --- Final Runtime Stage ---
FROM debian:bookworm-slim
ARG USE_MIRROR=auto
WORKDIR /app

# Conditionally use Aliyun mirror for APT
RUN if [ "$USE_MIRROR" = "true" ] || ( [ "$USE_MIRROR" = "auto" ] && ! timeout 3 bash -c "</dev/tcp/www.google.com/80" 2>/dev/null ); then \
        echo "Using Aliyun mirror for APT..."; \
        sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list.d/debian.sources || \
        sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list; \
    else \
        echo "Using default APT sources..."; \
    fi

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    libsqlite3-0 \
    ca-certificates \
    libgtk-3-0 \
    libwebkit2gtk-4.1-0 \
    libayatana-appindicator3-1 \
    librsvg2-2 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=backend-builder /app/src-tauri/target/release/antigravity_tools /app/antigravity-tools

# Copy frontend dist from builder
COPY --from=frontend-builder /app/dist /app/dist

# Set environment variables
ENV ABV_DIST_PATH=/app/dist
ENV RUST_LOG=info
ENV PORT=8045

# Expose the proxy/admin port
EXPOSE 8045

# Run the application in headless mode
ENTRYPOINT ["/app/antigravity-tools", "--headless"]
