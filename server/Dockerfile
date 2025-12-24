# ============================================
# Antigravity API Server - Multi-stage Build
# ============================================

# Stage 1: Build
FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

# 安装构建依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 复制 Cargo 文件
COPY Cargo.toml Cargo.lock* ./

# 创建虚拟 main.rs 用于缓存依赖
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# 构建依赖 (利用 Docker 层缓存)
RUN cargo build --release && rm -rf src

# 复制实际源代码
COPY src ./src

# 重新构建 (只编译应用代码)
RUN touch src/main.rs && cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 从构建阶段复制二进制文件
COPY --from=builder /app/target/release/antigravity-server /usr/local/bin/

# 创建数据目录
RUN mkdir -p /data/accounts /data/config /app/static

# 设置环境变量（代码中使用 PORT 和 HOST）
ENV PORT=8045
ENV HOST=0.0.0.0
ENV ANTIGRAVITY_ACCOUNTS_DIR=/data/accounts
ENV ANTIGRAVITY_CONFIG_DIR=/data/config
ENV ANTIGRAVITY_STATIC_DIR=/app/static
ENV RUST_LOG=antigravity_server=info

# 暴露端口
EXPOSE 8045

# 健康检查
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8045/health || exit 1

# 启动命令
CMD ["antigravity-server"]
