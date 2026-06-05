#!/bin/bash
# build.sh - 一键构建 url-monitor 镜像
# 支持自动版本提取、多标签、错误检查、强制重新编译

set -e

# ========== 配置 ==========
IMAGE_NAME="url-monitor"
DOCKERFILE="./dockerfile"
CARGO_TOML="./Cargo.toml"
DEFAULT_VERSION="latest"
FORCE_REBUILD=${FORCE_REBUILD:-false}  # 是否强制重建（清除缓存）
# =========================

# ========== 辅助函数 ==========
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1"
}

error() {
    echo "❌ $1" >&2
    exit 1
}

show_help() {
    cat << EOF
用法: ./build.sh [选项]

选项:
    -h, --help      显示帮助信息
    -f, --force     强制重建（清除 Docker 缓存）
    --no-cache      构建时不使用缓存

示例:
    ./build.sh              # 正常构建
    ./build.sh --force      # 强制重建（清除缓存）
    ./build.sh --no-cache   # 构建时不使用缓存
EOF
}
# =========================

# ========== 解析参数 ==========
NO_CACHE=""
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -f|--force)
            FORCE_REBUILD=true
            shift
            ;;
        --no-cache)
            NO_CACHE="--no-cache"
            shift
            ;;
        *)
            error "未知选项: $1\n使用 -h 查看帮助"
            ;;
    esac
done
# =========================

# ========== 强制重建 ==========
if [ "$FORCE_REBUILD" = true ]; then
    log "🧹 强制重建模式：清除 Docker 构建缓存..."
    docker builder prune -a -f
    log "✅ 缓存已清除"
fi
# =========================

# ========== 版本检测 ==========
if [ -f "$CARGO_TOML" ]; then
    VERSION=$(grep -E '^\s*version\s*=' "$CARGO_TOML" | head -n1 | sed 's/.*=\s*"\([^"]*\)".*/\1/')
    if [ -z "$VERSION" ]; then
        log "⚠️  无法从 $CARGO_TOML 提取版本号，使用默认标签: $DEFAULT_VERSION"
        VERSION="$DEFAULT_VERSION"
    else
        log "📦 检测到项目版本: v$VERSION"
    fi
else
    log "⚠️  未找到 $CARGO_TOML，使用默认标签: $DEFAULT_VERSION"
    VERSION="$DEFAULT_VERSION"
fi
# =========================

# ========== 文件检查 ==========
[ -f "$DOCKERFILE" ] || error "找不到 Dockerfile ($DOCKERFILE)"
[ -f "./src/main.rs" ] || error "找不到源码 (./src/main.rs)"
# =========================

# ========== 构建参数 ==========
TAGS=(
    "$IMAGE_NAME:$VERSION"
)
if [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    SHORT_TAG="${VERSION%.*}"
    TAGS+=("$IMAGE_NAME:$SHORT_TAG")
fi
TAGS+=("$IMAGE_NAME:latest")

BUILD_ARGS=()
while IFS= read -r tag; do
    BUILD_ARGS+=(-t "$tag")
done < <(printf '%s\n' "${TAGS[@]}")

# 添加 --no-cache 参数
if [ -n "$NO_CACHE" ]; then
    BUILD_ARGS+=("$NO_CACHE")
    log "⚠️  构建时将不使用缓存"
fi
# =========================

# ========== 执行构建 ==========
log "开始构建镜像（标签: ${TAGS[*]}）"

# 显示构建命令（便于调试）
if [ -n "$NO_CACHE" ]; then
    log "使用 --no-cache 参数，将完全重新编译"
fi

docker build \
  --network=host \
  -f "$DOCKERFILE" \
  "${BUILD_ARGS[@]}" \
  .

log "✅ 构建成功！镜像标签:"
for tag in "${TAGS[@]}"; do
    echo "   - $tag"
done

# 显示镜像大小
log "📊 镜像信息:"
docker images | grep -E "REPOSITORY|$IMAGE_NAME" | head -4

cat << EOF

💡 使用建议：
  1. 生产环境请使用固定版本标签（如 $IMAGE_NAME:$VERSION）
  2. 启动容器：
       docker run -p 8483:8483 --rm $IMAGE_NAME:$VERSION
  3. 或通过 docker-compose 启动：
       docker compose up -d
  4. 强制重建（清除缓存）：
       ./build.sh --force
EOF