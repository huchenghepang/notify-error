#!/bin/bash

# 文档参考：/docs/DEPLOY.md
# ==================== 配置加载 ====================
# 加载环境变量（如果存在）
if [ -f ".deploy.env" ]; then
    source .deploy.env
else
    echo "⚠️  未找到 .deploy.env 文件，使用脚本内默认值"
fi

APP_NAME=$(grep -E '^\s*name\s*=' ./Cargo.toml 2>/dev/null | head -n1 | sed 's/.*=\s*"\([^"]*\)".*/\1/' || echo "url-monitor")

SERVER_USER=${SERVER_USER:-root}
SERVER_HOST=${SERVER_HOST:-}
SERVER_PORT=${SERVER_PORT:-22}
HEALTH_CHECK_ENABLED=${HEALTH_CHECK_ENABLED:-false}
HEALTH_CHECK_URL=${HEALTH_CHECK_URL:-""}

BACKUP_DIR=${BACKUP_DIR:-"./backups"}
KEEP_BACKUPS=${KEEP_BACKUPS:-10}
TEST_PATH=${TEST_PATH:-/var/www/${APP_NAME}}
STAGING_PATH=${STAGING_PATH:-/var/www/${APP_NAME}-staging}
PRODUCTION_PATH=${PRODUCTION_PATH:-/var/www/${APP_NAME}}

# ==================== 新增：镜像配置 ====================
SKIP_BUILD=${SKIP_BUILD:-false}  # 跳过构建，使用现有镜像
IMAGE_SOURCE=${IMAGE_SOURCE:-"local"}  # local: 本地镜像, registry: 仓库镜像
REGISTRY_URL=${REGISTRY_URL:-""}  # 镜像仓库地址
IMAGE_TAG=${IMAGE_TAG:-"latest"}  # 镜像标签

if [ -z "$SERVER_HOST" ]; then
    error_exit "未配置 SERVER_HOST，请在 .deploy.env 中设置"
fi

# 日志配置
LOG_DIR=${LOG_DIR:-"./logs/deploy"}
LOG_LEVEL=${LOG_LEVEL:-"INFO"}
NO_CONSOLE=${NO_CONSOLE:-false}

# ==================== 函数定义 ====================
# 日志函数
log() {
    local level=$1
    shift
    local message="$*"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    
    case $LOG_LEVEL in
        DEBUG)
            echo "[$timestamp] [$level] $message"
            ;;
        INFO)
            if [[ "$level" != "DEBUG" ]]; then
                echo "[$timestamp] [$level] $message"
            fi
            ;;
        ERROR)
            if [[ "$level" == "ERROR" ]]; then
                echo "[$timestamp] [$level] $message"
            fi
            ;;
        *)
            echo "[$timestamp] [$level] $message"
            ;;
    esac
}

# 错误处理函数
error_exit() {
    log "ERROR" "$1"
    echo "❌ $1"
    exit 1
}

# 清理函数
cleanup() {
    log "INFO" "清理临时文件..."
    rm -f ${TEMP_REMOTE_SCRIPT} 2>/dev/null
    rm -f ${IMAGE_TAR} 2>/dev/null
}

# 健康检查函数
health_check() {
    if [ "${HEALTH_CHECK_ENABLED:-false}" != "true" ]; then
        log "INFO" "健康检查未启用，跳过检查"
        return 0
    fi
    
    if [ -z "$HEALTH_CHECK_URL" ]; then
        log "WARN" "健康检查已启用但未配置 HEALTH_CHECK_URL，跳过检查"
        return 0
    fi
    
    local max_attempts=${HEALTH_CHECK_ATTEMPTS:-30}
    local attempt=1
    local timeout=${HEALTH_CHECK_TIMEOUT:-5}
    local expected_status=${HEALTH_CHECK_EXPECTED_STATUS:-200}
    local fail_action=${HEALTH_CHECK_FAIL_ACTION:-"warn"}
    
    log "INFO" "========== 开始健康检查 =========="
    log "INFO" "检查地址: ${HEALTH_CHECK_URL}"
    log "INFO" "最大尝试次数: ${max_attempts}"
    log "INFO" "请求超时: ${timeout}秒"
    log "INFO" "期望状态码: ${expected_status}"
    
    if ! command -v curl &> /dev/null; then
        log "ERROR" "curl 命令未安装，无法执行健康检查"
        return 1
    fi
    
    log "INFO" "等待服务启动..."
    
    while [ $attempt -le $max_attempts ]; do
        local http_code=""
        local curl_exit_code=0
        
        if [ $((attempt % 5)) -eq 0 ] || [ $attempt -eq 1 ] || [ $attempt -eq $max_attempts ]; then
            log "INFO" "健康检查尝试 ${attempt}/${max_attempts}..."
        else
            log "DEBUG" "健康检查尝试 ${attempt}/${max_attempts}"
        fi
        
        http_code=$(curl -f -s -o /dev/null -w "%{http_code}" --max-time ${timeout} \
            "${HEALTH_CHECK_URL}" 2>/dev/null) || curl_exit_code=$?
        
        if [ $curl_exit_code -eq 0 ] && [ "$http_code" = "$expected_status" ]; then
            log "SUCCESS" "✅ 健康检查通过！"
            log "INFO" "  状态码: ${http_code}"
            log "INFO" "  总尝试次数: ${attempt}/${max_attempts}"
            return 0
        else
            log "DEBUG" "检查失败 - 状态码: ${http_code:-'无响应'}, 退出码: ${curl_exit_code}"
        fi
        
        if [ $attempt -lt $max_attempts ]; then
            local sleep_time=${HEALTH_CHECK_INTERVAL:-5}
            sleep ${sleep_time}
        fi
        
        ((attempt++))
    done
    
    log "ERROR" "❌ 健康检查失败！"
    log "ERROR" "  检查地址: ${HEALTH_CHECK_URL}"
    log "ERROR" "  总尝试次数: ${max_attempts}"
    
    case $fail_action in
        fail)
            log "ERROR" "根据配置，健康检查失败将中断部署"
            return 1
            ;;
        warn|skip|*)
            log "WARN" "根据配置，健康检查失败仅警告，继续部署"
            return 0
            ;;
    esac
}

# ==================== 新增：获取镜像 ====================
get_image() {
    if [ "$SKIP_BUILD" = true ]; then
        log "INFO" "跳过构建，使用现有镜像"
        
        if [ "$IMAGE_SOURCE" = "registry" ] && [ -n "$REGISTRY_URL" ]; then
            # 从仓库拉取镜像
            log "INFO" "从仓库拉取镜像: ${REGISTRY_URL}:${IMAGE_TAG}"
            if docker pull "${REGISTRY_URL}:${IMAGE_TAG}"; then
                IMAGE_TAG_FULL="${REGISTRY_URL}:${IMAGE_TAG}"
                log "INFO" "镜像拉取成功"
            else
                error_exit "镜像拉取失败"
            fi
        else
            # 使用本地镜像
            log "INFO" "使用本地镜像: ${APP_NAME}:${IMAGE_TAG}"
            if docker images | grep -q "${APP_NAME}.*${IMAGE_TAG}"; then
                IMAGE_TAG_FULL="${APP_NAME}:${IMAGE_TAG}"
                log "INFO" "找到本地镜像"
            else
                error_exit "本地镜像 ${APP_NAME}:${IMAGE_TAG} 不存在"
            fi
        fi
    else
        # 执行构建
        log "INFO" "开始 Docker 构建..."
        BUILD_LOG="${LOG_DIR}/build-${ENV}-${LOG_DATE}.log"
        
        if bash ./build.sh > "${BUILD_LOG}" 2>&1; then
            log "INFO" "Docker 镜像构建成功"
            IMAGE_TAG_FULL="${APP_NAME}:latest"
        else
            log "ERROR" "Docker 镜像构建失败，详情见: ${BUILD_LOG}"
            error_exit "Docker 镜像构建失败"
        fi
    fi
    
    # 获取版本号
    VERSION=$(docker inspect "${IMAGE_TAG_FULL}" | grep -m1 "org.opencontainers.image.version" | cut -d'"' -f4 || echo "unknown")
    if [ "$VERSION" = "unknown" ]; then
        VERSION=$(grep -E '^\s*version\s*=' ./Cargo.toml 2>/dev/null | head -n1 | sed 's/.*=\s*"\([^"]*\)".*/\1/' || echo "latest")
    fi
}

# ==================== 初始化 ====================
mkdir -p "${LOG_DIR}"
LOG_DATE=$(date +"%Y%m%d_%H%M%S")
LOG_FILE="${LOG_DIR}/deploy-${ENV:-unknown}-${LOG_DATE}.log"

if [ "$NO_CONSOLE" = true ]; then
    exec > "$LOG_FILE" 2>&1
else
    exec > >(tee -a "$LOG_FILE") 2>&1
fi

trap cleanup EXIT

declare -A ENV_PATHS
ENV_PATHS["test"]="$TEST_PATH"
ENV_PATHS["staging"]="$STAGING_PATH"
ENV_PATHS["production"]="$PRODUCTION_PATH"

for env in test staging production; do
    if [ -z "${ENV_PATHS[$env]}" ]; then
        error_exit "环境 $env 的路径未配置"
    fi
done

set -e

# ==================== 帮助信息 ====================
show_help() {
    cat << EOF
用法: $0 [选项]

选项:
  -e, --env ENV         部署环境 (test|staging|production)，默认: test
  -p, --path PATH       指定服务器部署路径（覆盖环境配置）
  -y, --yes             跳过服务器端部署确认（自动部署）
  --skip-build          跳过构建，使用现有镜像
  --image-tag TAG       指定镜像标签（默认: latest）
  --image-source SOURCE 镜像来源: local 或 registry（默认: local）
  --registry-url URL    镜像仓库地址（当 source=registry 时必需）
  --log-dir DIR         指定日志目录（默认: ./logs/deploy）
  --log-level LEVEL     设置日志级别（DEBUG|INFO|ERROR），默认: INFO
  --no-console          只记录到文件，不输出到控制台
  -h, --help            显示帮助信息

示例:
  $0                                    # 部署到 test 环境（构建）
  $0 --skip-build                       # 跳过构建，使用现有镜像
  $0 --skip-build --image-tag v1.0.0    # 使用指定版本镜像
  $0 --skip-build --image-source registry --registry-url myrepo/url-monitor  # 从仓库拉取
  $0 -e production --skip-build -y      # 生产环境自动部署（使用现有镜像）
  $0 -p /var/www/custom-path --skip-build  # 自定义路径

EOF
    exit 0
}

# ==================== 参数解析 ====================
ENV="test"
AUTO_DEPLOY=false
CUSTOM_PATH=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -e|--env)
            ENV="$2"
            shift 2
            ;;
        -p|--path)
            CUSTOM_PATH="$2"
            shift 2
            ;;
        -y|--yes)
            AUTO_DEPLOY=true
            shift
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --image-tag)
            IMAGE_TAG="$2"
            shift 2
            ;;
        --image-source)
            IMAGE_SOURCE="$2"
            shift 2
            ;;
        --registry-url)
            REGISTRY_URL="$2"
            shift 2
            ;;
        --log-dir)
            LOG_DIR="$2"
            shift 2
            ;;
        --log-level)
            LOG_LEVEL="$2"
            shift 2
            ;;
        --no-console)
            NO_CONSOLE=true
            shift
            ;;
        -h|--help)
            show_help
            ;;
        *)
            echo "❌ 未知参数: $1"
            show_help
            ;;
    esac
done

# 重新设置日志
mkdir -p "${LOG_DIR}"
LOG_DATE=$(date +"%Y%m%d_%H%M%S")
LOG_FILE="${LOG_DIR}/deploy-${ENV}-${LOG_DATE}.log"

if [ "$NO_CONSOLE" = true ]; then
    exec > "$LOG_FILE" 2>&1
else
    exec > >(tee -a "$LOG_FILE") 2>&1
fi

# ==================== 确定服务器路径 ====================
if [ -n "$CUSTOM_PATH" ]; then
    SERVER_PATH="$CUSTOM_PATH"
    echo "📁 使用自定义路径: ${SERVER_PATH}"
else
    if [ -n "${ENV_PATHS[$ENV]}" ]; then
        SERVER_PATH="${ENV_PATHS[$ENV]}"
        echo "📁 使用环境 [${ENV}] 路径: ${SERVER_PATH}"
    else
        error_exit "未知环境 '${ENV}'，可用环境: test, staging, production"
    fi
fi

BACKUP_DIR="${BACKUP_DIR}/${ENV}"
mkdir -p "${BACKUP_DIR}"

# ==================== 打印配置信息 ====================
echo "=========================================="
echo "📋 部署配置信息"
echo "=========================================="
echo "🧑‍💻 当前部署环境: ${ENV}"
echo "🎯 当前部署路径: ${SERVER_PATH}"
echo "📝 日志文件: ${LOG_FILE}"
echo ""
echo "🔧 服务器配置:"
echo "   用户: ${SERVER_USER}"
echo "   主机: ${SERVER_HOST}"
echo "   端口: ${SERVER_PORT}"
echo ""
echo "🐳 Docker 配置:"
echo "   跳过构建: ${SKIP_BUILD}"
if [ "$SKIP_BUILD" = true ]; then
    echo "   镜像来源: ${IMAGE_SOURCE}"
    echo "   镜像标签: ${IMAGE_TAG}"
    if [ "$IMAGE_SOURCE" = "registry" ]; then
        echo "   仓库地址: ${REGISTRY_URL}"
    fi
fi
echo ""
echo "💾 备份配置:"
echo "   备份目录: ${BACKUP_DIR}"
echo "   保留备份数: ${KEEP_BACKUPS}"
echo ""
echo "📦 应用信息:"
echo "   应用名称: ${APP_NAME}"
echo "=========================================="
echo ""

log "INFO" "========== 开始部署流程 =========="
log "INFO" "环境: ${ENV}"
log "INFO" "路径: ${SERVER_PATH}"
log "INFO" "跳过构建: ${SKIP_BUILD}"

# ==================== SSH 连接测试 ====================
log "INFO" "测试 SSH 连接..."
if ! ssh -p ${SERVER_PORT} -o ConnectTimeout=10 ${SERVER_USER}@${SERVER_HOST} "echo OK" 2>/dev/null; then
    error_exit "无法连接到服务器 ${SERVER_HOST}:${SERVER_PORT}"
fi
log "INFO" "SSH 连接成功"

# ==================== 获取镜像 ====================
get_image

# ==================== 导出镜像 ====================
DATE=$(date +"%Y%m%d_%H%M%S")
IMAGE_TAR="${APP_NAME}-${ENV}-v${VERSION}-${DATE}.tar"

log "INFO" "版本: v${VERSION}"
log "INFO" "导出镜像: ${IMAGE_TAG_FULL} -> ${IMAGE_TAR}"

echo "📤 导出 Docker 镜像..."
if docker save -o "${IMAGE_TAR}" "${IMAGE_TAG_FULL}" 2>/dev/null; then
    log "INFO" "镜像导出成功: ${IMAGE_TAR}"
else
    error_exit "镜像导出失败"
fi

if [ ! -f "${IMAGE_TAR}" ]; then
    error_exit "镜像文件 ${IMAGE_TAR} 未生成"
fi
log "INFO" "构建产物检查通过"

# ==================== 备份 ====================
echo "💾 保存本地备份到: ${BACKUP_DIR}/${IMAGE_TAR}"
cp "${IMAGE_TAR}" "${BACKUP_DIR}/${IMAGE_TAR}"
log "INFO" "本地备份已保存: ${BACKUP_DIR}/${IMAGE_TAR}"

# ==================== 检查服务器目录 ====================
echo "📁 确保服务器目录存在..."
ssh -p ${SERVER_PORT} ${SERVER_USER}@${SERVER_HOST} "mkdir -p ${SERVER_PATH}"
log "INFO" "服务器目录已准备: ${SERVER_PATH}"

# ==================== 上传文件 ====================
echo "📤 上传文件到服务器..."
log "INFO" "开始上传镜像文件: ${IMAGE_TAR}"

if scp -P ${SERVER_PORT} "${IMAGE_TAR}" "${SERVER_USER}@${SERVER_HOST}:${SERVER_PATH}/"; then
    log "INFO" "镜像文件上传成功"
else
    error_exit "镜像文件上传失败"
fi

# 上传配置文件
for file in docker-compose.yml .env config.json; do
    if [ -f "$file" ]; then
        echo "📤 上传 $file..."
        scp -P ${SERVER_PORT} "$file" "${SERVER_USER}@${SERVER_HOST}:${SERVER_PATH}/" 2>/dev/null || \
            log "WARN" "$file 上传失败"
    fi
done

# ==================== 部署 ====================
if [ "$AUTO_DEPLOY" = true ]; then
    DEPLOY_ANSWER="y"
else
    echo "🔧 是否在服务器上自动部署? (y/n)"
    read -r DEPLOY_ANSWER
fi

if [ "$DEPLOY_ANSWER" = "y" ]; then
    log "INFO" "开始 Docker 自动部署"
    
    ssh -p ${SERVER_PORT} ${SERVER_USER}@${SERVER_HOST} << EOF
        set -e
        cd ${SERVER_PATH}
        
        echo "🐳 加载 Docker 镜像..."
        docker load -i ${IMAGE_TAR}
        
        echo "🛑 停止旧容器..."
        docker-compose down --remove-orphans 2>/dev/null || true
        
        echo "🚀 启动新容器..."
        docker-compose up -d
        
        echo "🧹 清理镜像文件..."
        rm -f ${IMAGE_TAR}
        
        echo "✅ ${ENV} 环境 Docker 部署完成"
EOF
    
    if [ $? -eq 0 ]; then
        log "INFO" "Docker 部署成功"
        sleep 3
        health_check
    else
        error_exit "Docker 部署失败"
    fi
else
    echo ""
    echo "⚠️  文件已上传但未部署: ${SERVER_PATH}/${IMAGE_TAR}"
    log "INFO" "用户选择不自动部署"
    
    echo "是否删除服务器上的文件？(y/n)"
    read -r CLEANUP_FILE
    
    if [[ $CLEANUP_FILE =~ ^[Yy]$ ]]; then
        ssh -p ${SERVER_PORT} ${SERVER_USER}@${SERVER_HOST} "rm -f ${SERVER_PATH}/${IMAGE_TAR}"
        echo "✅ 已清理服务器上的文件"
    else
        echo "📦 文件已保留，请手动处理"
    fi
fi

# ==================== 清理旧备份 ====================
log "INFO" "清理旧备份（保留最近 ${KEEP_BACKUPS} 个）"
cd ${BACKUP_DIR}
ls -t *.tar 2>/dev/null | tail -n +$((KEEP_BACKUPS + 1)) | xargs -r rm -f
cd - > /dev/null

# ==================== 完成 ====================
echo ""
echo "=========================================="
echo "✅ 部署完成!"
echo "🌐 环境: ${ENV}"
echo "📁 路径: ${SERVER_PATH}"
echo "📦 备份: ${BACKUP_DIR}/${IMAGE_TAR}"
echo "📝 日志: ${LOG_FILE}"
echo "=========================================="

log "INFO" "========== 部署流程完成 =========="