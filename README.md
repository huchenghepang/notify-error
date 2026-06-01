# URL 监控工具

一个用于监控网站可用性的 Rust 应用程序，支持飞书机器人通知和自定义配置。

## 功能特性

- 监控多个网站的可用性
- 支持自定义HTTP方法、状态码、超时等
- 飞书机器人通知（支持签名验证）
- 自定义报警信息模板
- 关键词内容检查
- 故障阈值控制（避免短暂波动）
- 恢复通知

## 安装要求

- Rust 1.70+
- Cargo

## 快速开始

### 1. 克隆项目

```bash
git clone <repository-url>
cd notify-error
```

### 2. 安装依赖

```bash
cargo build
```

### 3. 配置环境变量

复制 `.env.example` 并创建 `.env` 文件：

```bash
cp .env.example .env
```

#### 环境变量说明

| 环境变量 | 类型 | 默认值 | 必须 | 说明 |
|----------|------|--------|------|------|
| `CHECK_INTERVAL_SECS` | 数字 | 30 | 否 | 检查间隔（秒） |
| `FAILURE_THRESHOLD` | 数字 | 1 | 否 | 失败阈值（连续失败几次后发送通知） |
| `RECOVERY_NOTIFICATION` | 布尔 | true | 否 | 是否发送恢复通知 |
| `FEISHU_WEBHOOK_URL` | 字符串 | - | 是 | 飞书机器人 webhook URL |
| `FEISHU_BOT_SECRET` | 字符串 | - | 否 | 飞书机器人密钥（如需签名验证） |
| `FEISHU_USER_IDS` | 字符串 | - | 否 | 需要提及的用户ID列表（逗号分隔） |

#### 环境变量示例

编辑 `.env` 文件：

```env
# 检查间隔（秒）
CHECK_INTERVAL_SECS=30

# 失败阈值（连续失败几次后发送通知）
FAILURE_THRESHOLD=3

# 是否发送恢复通知
RECOVERY_NOTIFICATION=true

# 飞书机器人 webhook URL
FEISHU_WEBHOOK_URL=https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-url

# 飞书机器人密钥（可选，如需签名验证）
FEISHU_BOT_SECRET=your-bot-secret

# 需要提及的用户ID（可选，多个用户用逗号分隔）
FEISHU_USER_IDS=ou_xxxxxxxxx,ou_yyyyyyyyy
```

#### 示例配置文件

创建 `.env.production` 用于生产环境：

```env
CHECK_INTERVAL_SECS=60
FAILURE_THRESHOLD=5
RECOVERY_NOTIFICATION=true
FEISHU_WEBHOOK_URL=https://open.feishu.cn/open-apis/bot/v2/hook/production-webhook-url
FEISHU_BOT_SECRET=production-bot-secret
FEISHU_USER_IDS=ou_xxxxxxxxx
```

创建 `.env.development` 用于开发环境：

```env
CHECK_INTERVAL_SECS=10
FAILURE_THRESHOLD=2
RECOVERY_NOTIFICATION=false
FEISHU_WEBHOOK_URL=https://open.feishu.cn/open-apis/bot/v2/hook/dev-webhook-url
FEISHU_BOT_SECRET=dev-bot-secret
```

### 4. 配置监控URL

创建 `config.json` 文件来定义要监控的URL：

```json
{
  "urls": [
    {
      "url": "https://www.example.com",
      "expected_status": 200,
      "expected_keyword": "Example Domain",
      "timeout_secs": 10,
      "method": "GET",
      "headers": {
        "User-Agent": "Mozilla/5.0 (compatible; URL Monitor)"
      },
      "custom_alert_message": "🚨 网站 {url} 访问失败！\n状态码: {status_code}\n错误信息: {error_message}\n响应时间: {response_time}ms\n时间: {timestamp}"
    }
  ]
}
```

#### 配置项说明

- `url`: 要监控的URL
- `expected_status`: 期望的HTTP状态码（可选，默认为200-299）
- `expected_keyword`: 期望在响应内容中包含的关键词（可选）
- `timeout_secs`: 请求超时时间（秒）
- `method`: HTTP方法（GET, POST, HEAD等）
- `headers`: 自定义请求头（可选）
- `custom_alert_message`: 自定义报警信息模板（可选）

#### 自定义报警信息模板变量

- `{url}`: 目标URL
- `{status_code}`: HTTP状态码
- `{error_message}`: 错误信息
- `{response_time}`: 响应时间（毫秒）
- `{timestamp}`: 当前时间戳

### 5. 运行应用

```bash
cargo run
```

## 配置详解

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CHECK_INTERVAL_SECS` | 30 | 检查间隔（秒） |
| `FAILURE_THRESHOLD` | 1 | 失败阈值 |
| `RECOVERY_NOTIFICATION` | true | 是否发送恢复通知 |
| `FEISHU_WEBHOOK_URL` | - | 飞书机器人webhook URL（必须） |
| `FEISHU_BOT_SECRET` | - | 飞书机器人密钥（可选） |
| `FEISHU_USER_IDS` | - | 需要提及的用户ID列表 |

### URL配置选项

支持多种HTTP方法和自定义配置：

```json
{
  "urls": [
    {
      "url": "https://api.example.com/health",
      "expected_status": 200,
      "expected_keyword": "healthy",
      "timeout_secs": 15,
      "method": "GET",
      "headers": {
        "Authorization": "Bearer token",
        "Custom-Header": "value"
      },
      "custom_alert_message": "🔴 API {url} 异常！状态: {status_code}, 错误: {error_message}"
    },
    {
      "url": "https://example.com/api/check",
      "expected_status": 201,
      "timeout_secs": 10,
      "method": "POST",
      "headers": {
        "Content-Type": "application/json"
      }
    }
  ]
}
```

## 飞书机器人设置

1. 在飞书群聊中添加机器人
2. 选择"自定义机器人"
3. 设置机器人名称和头像
4. 获取 webhook URL 并填写到 `.env` 文件中
5. 如需安全验证，设置密钥并在 `.env` 中配置

## 使用场景

- 网站健康检查
- API服务监控
- 内部系统可用性监控
- 第三方服务状态监控

## 故障排除

### 常见问题

1. **无法加载 .env 文件**
   - 确保 `.env` 文件位于项目根目录
   - 检查文件权限

2. **监控URL无法访问但显示正常**
   - 检查 `expected_status` 和 `expected_keyword` 设置
   - 验证网络连接

3. **飞书通知发送失败**
   - 检查 webhook URL 是否正确
   - 验证密钥设置

## Windows平台编译和使用

### 编译为Windows可执行文件

如果您在Linux/macOS上交叉编译Windows版本：

1. 安装Windows目标工具链：
   ```bash
   rustup target add x86_64-pc-windows-gnu
   ```

2. 安装Windows交叉编译工具（以Ubuntu为例）：
   ```bash
   sudo apt-get install gcc-mingw-w64-x86-64
   ```

3. 编译Windows版本：
   ```bash
   cargo build --release --target x86_64-pc-windows-gnu
   ```
   
   编译后的可执行文件将在 `target/x86_64-pc-windows-gnu/release/url-monitor.exe`

### 在Windows上直接编译

1. 在Windows上安装Rust：
   - 下载并运行 https://win.rustup.rs/
   - 按照提示完成安装

2. 克隆或下载项目源码：
   ```cmd
   git clone <repository-url>
   cd notify-error
   ```

3. 编译项目：
   ```cmd
   cargo build --release
   ```
   
   编译后的可执行文件将在 `target/release/url-monitor.exe`

### Windows上运行

1. 创建配置文件 `.env` 和 `config.json`（参考前面的配置说明）
2. 在命令提示符(CMD)中运行：
   ```cmd
   url-monitor.exe
   ```

   在PowerShell中运行（注意需要使用 `.\` 前缀）：
   ```powershell
   .\url-monitor.exe
   ```

   如果遇到执行策略错误，请先运行以下命令允许本地脚本执行：
   ```powershell
   Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
   ```

## 开机自启和后台执行

### Linux系统

#### 使用systemd服务（推荐）

1. 创建systemd服务文件：
   ```bash
   sudo nano /etc/systemd/system/url-monitor.service
   ```

2. 添加以下内容（根据您的实际路径调整）：
   ```ini
   [Unit]
   Description=URL Monitor Service
   After=network.target
   StartLimitIntervalSec=0

   [Service]
   Type=simple
   Restart=always
   RestartSec=1
   User=your_username
   WorkingDirectory=/path/to/your/url-monitor
   ExecStart=/path/to/your/url-monitor/target/release/url-monitor
   Environment=ENV_FILE_PATH=/path/to/your/url-monitor/.env

   [Install]
   WantedBy=multi-user.target
   ```

3. 重新加载systemd并启用服务：
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable url-monitor
   sudo systemctl start url-monitor
   ```

4. 检查服务状态：
   ```bash
   sudo systemctl status url-monitor
   ```

#### 使用nohup后台运行

```bash
# 在后台运行并输出日志到文件
nohup ./target/release/url-monitor > monitor.log 2>&1 &
```

#### 使用screen/tmux

```bash
# 安装screen
sudo apt install screen

# 创建一个新的screen会话
screen -S url-monitor

# 运行程序
./target/release/url-monitor

# 按Ctrl+A, 然后按D分离会话
```

### Windows系统

#### 使用任务计划程序

1. 打开"任务计划程序"
2. 点击"创建基本任务"
3. 设置触发器为"计算机启动时"
4. 选择"启动程序"
5. 程序路径指向 `url-monitor.exe`
6. 在"起始于"字段中指定程序所在的目录

#### 创建批处理脚本

创建 `start-monitor.bat`：
```batch
@echo off
cd /d "C:\path\to\your\url-monitor"
start /min url-monitor.exe
```

将此脚本添加到Windows启动文件夹：
1. 按 Win+R，输入 `shell:startup`
2. 将批处理脚本复制到打开的文件夹中

#### 使用NSSM (Non-Sucking Service Manager)

1. 下载NSSM: https://nssm.cc/download
2. 安装服务：
   ```cmd
   nssm install URLMonitor "C:\path\to\your\url-monitor.exe"
   nssm start URLMonitor
   ```

### Docker部署（可选）

创建 `Dockerfile`：
```Dockerfile
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY target/release/url-monitor /app/url-monitor
COPY .env /app/.env
COPY config.json /app/config.json

WORKDIR /app

CMD ["./url-monitor"]
```

创建 `docker-compose.yml`：
```yaml
version: '3'
services:
  url-monitor:
    build: .
    restart: always
    volumes:
      - ./config.json:/app/config.json
      - ./logs:/app/logs
```

## 许可证

MIT License