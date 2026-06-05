# 构建阶段
FROM rust:latest AS builder

WORKDIR /app

# ============================================
# 配置 RsProxy 的 Sparse 镜像源
# ============================================
RUN printf '[source.crates-io]\n\
replace-with = "rsproxy-sparse"\n\n\
[source.rsproxy]\n\
registry = "https://rsproxy.cn/crates.io-index"\n\n\
[source.rsproxy-sparse]\n\
registry = "sparse+https://rsproxy.cn/index/"\n\n\
[net]\n\
git-fetch-with-cli = true\n' > /usr/local/cargo/config.toml
# ============================================

# 直接复制所有源码
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# 直接编译（不使用预编译）
RUN cargo build --release

# 运行阶段
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 从构建阶段复制二进制文件
COPY --from=builder /app/target/release/url-monitor /usr/local/bin/url-monitor

EXPOSE 8483

CMD ["/usr/local/bin/url-monitor"]