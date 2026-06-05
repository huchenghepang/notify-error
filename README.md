# URL 监控工具

一个 Rust 编写的网站可用性监控工具，定时检查目标 URL 状态，异常时通过飞书机器人发送告警通知。

## 功能特性

- 监控多个 URL 的可用性
- 支持自定义 HTTP 方法、请求头、超时时间
- 飞书机器人通知（支持签名验证）
- 自定义告警消息模板
- 关键词内容检查
- 故障阈值控制（避免误报）
- 健康恢复通知
- 并行检查支持
- Docker 一键部署

---

## Docker 部署（推荐）

### 1. 配置环境变量

```bash
cp .env.example .env
```

编辑 `.env`，填入飞书机器人的 webhook 地址和密钥：

```env
CHECK_INTERVAL_SECS=30
FAILURE_THRESHOLD=3
RECOVERY_NOTIFICATION=true
FEISHU_WEBHOOK_URL=https://open.feishu.cn/open-apis/bot/v2/hook/xxx
FEISHU_BOT_SECRET=your-secret
FEISHU_USER_IDS=ou_xxx,ou_yyy
```

### 2. 配置监控 URL

编辑 `config.json`，添加要监控的网址：

```json
{
  "urls": [
    {
      "url": "https://www.example.com",
      "expected_status": 200,
      "timeout_secs": 10,
      "method": "GET"
    },
    {
      "url": "https://api.example.com/health",
      "expected_status": 200,
      "expected_keyword": "ok",
      "timeout_secs": 5,
      "method": "GET"
    }
  ]
}
```

### 3. 构建并启动

```bash
# 构建镜像并启动
docker compose up -d --build

# 查看日志
docker compose logs -f

# 停止
docker compose down
```

### 4. 构建脚本

也可以使用 `build.sh` 一键构建镜像：

```bash
chmod +x build.sh
./build.sh
```

---

## 本地开发运行

### 环境要求

- Rust 1.70+

### 运行

```bash
# 克隆项目
git clone <repo-url> && cd notify-error

# 配置
cp .env.example .env
# 编辑 .env 和 config.json

# 编译运行
cargo run

# 或直接运行 release 版本
cargo build --release
./target/release/url-monitor
```

---

## 配置说明

### 环境变量

| 变量 | 默认值 | 必填 | 说明 |
|------|--------|------|------|
| `CHECK_INTERVAL_SECS` | 30 | 否 | 检查间隔（秒） |
| `FAILURE_THRESHOLD` | 1 | 否 | 连续失败 N 次后才发告警 |
| `RECOVERY_NOTIFICATION` | true | 否 | 服务恢复时是否通知 |
| `PARALLEL_CHECKS` | true | 否 | 是否并行检查多个 URL |
| `MAX_PARALLEL_CHECKS` | 5 | 否 | 最大并行检查数 |
| `FEISHU_WEBHOOK_URL` | - | **是** | 飞书机器人 webhook 地址 |
| `FEISHU_BOT_SECRET` | - | 否 | 飞书机器人签名密钥 |
| `FEISHU_USER_IDS` | - | 否 | 要 @ 的用户 ID，逗号分隔 |

### config.json 配置项

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 监控目标 URL |
| `expected_status` | number | 否 | 期望的 HTTP 状态码，默认 2xx |
| `expected_keyword` | string | 否 | 响应内容中必须包含的关键词 |
| `timeout_secs` | number | 否 | 请求超时（秒），默认 10 |
| `method` | string | 否 | HTTP 方法，默认 GET |
| `headers` | object | 否 | 自定义请求头 |
| `request_body` | string | 否 | 请求体（POST/PUT 等） |
| `custom_alert_message` | string | 否 | 自定义告警消息模板 |

### 告警消息模板变量

自定义告警消息中可使用以下变量：

- `{url}` — 目标 URL
- `{status_code}` — HTTP 状态码
- `{error_message}` — 错误详情
- `{response_time}` — 响应时间（毫秒）
- `{timestamp}` — 当前时间

示例：

```json
{
  "custom_alert_message": "🚨 {url} 挂了！\n状态码: {status_code}\n错误: {error_message}\n时间: {timestamp}"
}
```

---

## 飞书机器人配置

1. 在飞书群聊中点击「设置」→「群机器人」→「添加机器人」
2. 选择「自定义机器人」
3. 设置名称和头像
4. 复制 webhook URL 填入 `.env` 的 `FEISHU_WEBHOOK_URL`
5. （可选）开启签名校验，将密钥填入 `FEISHU_BOT_SECRET`

---

## 后台运行

### systemd（推荐）

```bash
sudo tee /etc/systemd/system/url-monitor.service <<EOF
[Unit]
Description=URL Monitor
After=network.target

[Service]
Type=simple
Restart=always
RestartSec=5
WorkingDirectory=/opt/url-monitor
ExecStart=/opt/url-monitor/url-monitor

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now url-monitor
```

### Docker 后台运行

```bash
docker compose up -d
```

---

## 项目结构

```
notify-error/
├── src/
│   ├── main.rs           # 入口、配置解析、监控循环
│   ├── lib.rs            # 库入口
│   └── feishu_client.rs  # 飞书通知模块
├── config.json           # URL 监控配置
├── .env                  # 环境变量（不提交 Git）
├── .env.example          # 环境变量示例
├── dockerfile            # Docker 镜像构建文件
├── docker-compose.yml    # Docker Compose 部署配置
├── build.sh              # 一键构建脚本
└── Cargo.toml
```

## License

MIT