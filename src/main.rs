use anyhow::{anyhow, Result};
use chrono::Local;
use hmac::{Hmac, Mac};
use reqwest::blocking::Client;
use serde_json::json;
use sha2::Sha256;
use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::Duration;

// URL 监控配置
#[derive(Debug, Clone)]
struct UrlMonitorConfig {
    url: String,                          // 要监控的 URL
    expected_status: Option<u16>,         // 期望的 HTTP 状态码（None 表示 200-299）
    expected_keyword: Option<String>,     // 期望在响应内容中包含的关键词
    timeout_secs: u64,                    // 请求超时时间
    method: String,                       // HTTP 方法（GET, POST, HEAD 等）
    headers: HashMap<String, String>,     // 自定义请求头
    custom_alert_message: Option<String>, // 自定义报警信息
}

// 全局配置
struct Config {
    check_interval_secs: u64,      // 检查间隔（秒）
    urls: Vec<UrlMonitorConfig>,   // 要监控的 URL 列表
    feishu_webhook_url: String,    // 飞书机器人 webhook URL
    feishu_secret: Option<String>, // 飞书机器人密钥（可选）
    feishu_user_ids: Vec<String>,  // 要通知的用户 ID 列表
    failure_threshold: u32,        // 失败多少次后才通知（避免短暂波动）
    recovery_notification: bool,   // 是否发送恢复通知
}

impl Config {
    fn from_env() -> Result<Self> {
        dotenv::dotenv().map_err(|e| anyhow!("无法加载 .env 文件: {}", e))?;

        let check_interval_secs = std::env::var("CHECK_INTERVAL_SECS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .map_err(|_| anyhow!("无效的检查间隔"))?;

        let feishu_webhook_url = std::env::var("FEISHU_WEBHOOK_URL")
            .map_err(|_| anyhow!("请设置 FEISHU_WEBHOOK_URL 环境变量"))?;

        let feishu_secret = std::env::var("FEISHU_BOT_SECRET").ok();

        let feishu_user_ids = std::env::var("FEISHU_USER_IDS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let failure_threshold = std::env::var("FAILURE_THRESHOLD")
            .unwrap_or_else(|_| "1".to_string())
            .parse()
            .unwrap_or(1);

        let recovery_notification = std::env::var("RECOVERY_NOTIFICATION")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap_or(true);

        // 解析 URL 配置
        let urls = Self::parse_urls_config()?;

        Ok(Config {
            check_interval_secs,
            urls,
            feishu_webhook_url,
            feishu_secret,
            feishu_user_ids,
            failure_threshold,
            recovery_notification,
        })
    }

    fn parse_urls_config() -> Result<Vec<UrlMonitorConfig>> {
        // 从 config.json 文件读取配置
        let config_content =
            fs::read_to_string("config.json").map_err(|_| anyhow!("无法读取 config.json 文件"))?;

        let config_json: serde_json::Value = serde_json::from_str(&config_content)
            .map_err(|e| anyhow!("解析 config.json 失败: {}", e))?;

        let urls_array = config_json["urls"]
            .as_array()
            .ok_or_else(|| anyhow!("config.json 中缺少 urls 数组"))?;

        let mut urls = Vec::new();

        for config in urls_array {
            let url = config["url"]
                .as_str()
                .ok_or_else(|| anyhow!("URL 配置缺少 url 字段"))?
                .to_string();

            let expected_status = config["expected_status"].as_u64().map(|s| s as u16);
            let expected_keyword = config["expected_keyword"].as_str().map(String::from);
            let timeout_secs = config["timeout_secs"].as_u64().unwrap_or(10);
            let method = config["method"].as_str().unwrap_or("GET").to_string();
            let custom_alert_message = config["custom_alert_message"].as_str().map(String::from);

            let mut headers = HashMap::new();
            if let Some(headers_obj) = config["headers"].as_object() {
                for (key, value) in headers_obj {
                    if let Some(value_str) = value.as_str() {
                        headers.insert(key.clone(), value_str.to_string());
                    }
                }
            }

            urls.push(UrlMonitorConfig {
                url,
                expected_status,
                expected_keyword,
                timeout_secs,
                method,
                headers,
                custom_alert_message,
            });
        }

        if urls.is_empty() {
            return Err(anyhow!("至少需要配置一个 URL"));
        }

        Ok(urls)
    }
}

// 检查结果
#[derive(Debug)]
struct CheckResult {
    url: String,
    success: bool,
    status_code: Option<u16>,
    error_message: Option<String>,
    response_time_ms: u64,
}

// 检查 URL 可用性
fn check_url(client: &Client, config: &UrlMonitorConfig) -> CheckResult {
    let start = std::time::Instant::now();

    // 构建请求
    let mut request_builder = match config.method.to_uppercase().as_str() {
        "GET" => client.get(&config.url),
        "POST" => client.post(&config.url),
        "HEAD" => client.head(&config.url),
        _ => client.get(&config.url),
    };

    // 添加自定义 headers
    for (key, value) in &config.headers {
        request_builder = request_builder.header(key, value);
    }

    // 发送请求
    let response = request_builder
        .timeout(Duration::from_secs(config.timeout_secs))
        .send();

    let response_time_ms = start.elapsed().as_millis() as u64;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let status_code = status.as_u16();

            // 检查状态码
            let status_ok = match config.expected_status {
                Some(expected) => status_code == expected,
                None => status.is_success(),
            };

            if !status_ok {
                return CheckResult {
                    url: config.url.clone(),
                    success: false,
                    status_code: Some(status_code),
                    error_message: Some(format!(
                        "意外的状态码: {} (期望: {})",
                        status_code,
                        config
                            .expected_status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "2xx".to_string())
                    )),
                    response_time_ms,
                };
            }

            // 检查关键词
            if let Some(keyword) = &config.expected_keyword {
                match resp.text() {
                    Ok(body) => {
                        if !body.contains(keyword) {
                            return CheckResult {
                                url: config.url.clone(),
                                success: false,
                                status_code: Some(status_code),
                                error_message: Some(format!("响应中未找到关键词: '{}'", keyword)),
                                response_time_ms,
                            };
                        }
                    }
                    Err(e) => {
                        return CheckResult {
                            url: config.url.clone(),
                            success: false,
                            status_code: Some(status_code),
                            error_message: Some(format!("读取响应体失败: {}", e)),
                            response_time_ms,
                        };
                    }
                }
            }

            CheckResult {
                url: config.url.clone(),
                success: true,
                status_code: Some(status_code),
                error_message: None,
                response_time_ms,
            }
        }
        Err(e) => CheckResult {
            url: config.url.clone(),
            success: false,
            status_code: None,
            error_message: Some(e.to_string()),
            response_time_ms,
        },
    }
}

// 生成飞书 Webhook 签名
fn generate_signature(secret: &str) -> (String, String) {
    let timestamp = Local::now().timestamp();
    let ts = timestamp.to_string();

    let str_to_sign = format!("{}\n{}", ts, secret);

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC key is valid");
    mac.update(str_to_sign.as_bytes());
    let result = mac.finalize();
    let sign = hex::encode(result.into_bytes());

    (ts, sign)
}

// 发送飞书通知
fn send_feishu_notification(
    config: &Config,
    results: &[CheckResult],
    is_recovery: bool,
) -> Result<()> {
    let client = Client::new();

    let (title, _color, content_text) = if is_recovery {
        ("✅ 服务恢复通知", "green", "以下服务已恢复正常")
    } else {
        ("🚨 服务异常告警", "red", "以下服务出现异常")
    };

    // 构建详细的消息内容
    let mut details = String::new();
    for result in results {
        // 查找对应的URL配置以获取自定义消息
        if let Some(url_config) = config.urls.iter().find(|c| c.url == result.url) {
            if let Some(ref custom_msg) = url_config.custom_alert_message {
                // 使用自定义消息并进行变量替换
                let custom_message = custom_msg
                    .replace("{url}", &result.url)
                    .replace(
                        "{status_code}",
                        &result
                            .status_code
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "N/A".to_string()),
                    )
                    .replace(
                        "{error_message}",
                        &result.error_message.as_deref().unwrap_or("无错误信息"),
                    )
                    .replace("{response_time}", &result.response_time_ms.to_string())
                    .replace(
                        "{timestamp}",
                        &Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    );

                details.push_str(&format!("\n\n{}", custom_message));
            } else {
                // 使用默认消息格式
                details.push_str(&format!("\n\n**{}**", result.url));
                if let Some(status) = result.status_code {
                    details.push_str(&format!("\n- 状态码: {}", status));
                }
                if let Some(err) = &result.error_message {
                    details.push_str(&format!("\n- 错误: {}", err));
                }
                details.push_str(&format!("\n- 响应时间: {}ms", result.response_time_ms));
            }
        } else {
            // 使用默认消息格式
            details.push_str(&format!("\n\n**{}**", result.url));
            if let Some(status) = result.status_code {
                details.push_str(&format!("\n- 状态码: {}", status));
            }
            if let Some(err) = &result.error_message {
                details.push_str(&format!("\n- 错误: {}", err));
            }
            details.push_str(&format!("\n- 响应时间: {}ms", result.response_time_ms));
        }
    }

    let at_users = if !config.feishu_user_ids.is_empty() {
        let ats: Vec<String> = config
            .feishu_user_ids
            .iter()
            .map(|id| format!("<at id={}></at>", id))
            .collect();
        format!("\n\n{}", ats.join(" "))
    } else {
        String::new()
    };

    let message = format!(
        "{}\n\n⏰ 时间: {}\n{}",
        title,
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        content_text
    );

    let content = json!({
        "msg_type": "post",
        "content": {
            "post": {
                "zh_cn": {
                    "title": title,
                    "content": [
                        [
                            {
                                "tag": "text",
                                "text": format!("{}{}{}", message, details, at_users)
                            }
                        ]
                    ]
                }
            }
        }
    });

    // 构建请求
    let mut request_builder = client.post(&config.feishu_webhook_url);

    // 如果设置了密钥，则添加签名
    if let Some(ref secret) = config.feishu_secret {
        let (timestamp, signature) = generate_signature(secret);
        request_builder = request_builder
            .header("Timestamp", timestamp)
            .header("Sign", signature);
    }

    let response = request_builder.json(&content).send()?;

    if response.status().is_success() {
        println!(
            "[{}] 📤 飞书通知发送成功",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        Ok(())
    } else {
        let error_text = response.text().unwrap_or_default();
        Err(anyhow!("飞书通知发送失败: {}", error_text))
    }
}

// URL 状态追踪器
#[derive(Debug)]
struct UrlStatus {
    consecutive_failures: u32,
    last_success: bool,
    last_notification_time: Option<chrono::DateTime<Local>>,
}

impl UrlStatus {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_success: true,
            last_notification_time: None,
        }
    }
}

fn main() -> Result<()> {
    println!("🚀 启动 URL 监控服务");
    println!("{}", "=".repeat(60));

    let config = Config::from_env()?;
    println!("📋 配置信息:");
    println!("  - 检查间隔: {} 秒", config.check_interval_secs);
    println!("  - 失败阈值: {} 次", config.failure_threshold);
    println!("  - 恢复通知: {}", config.recovery_notification);
    println!("  - 监控 URL 数量: {}", config.urls.len());
    println!(
        "  - 飞书机器人密钥: {}",
        if config.feishu_secret.is_some() {
            "已配置"
        } else {
            "未配置"
        }
    );

    for (i, url_config) in config.urls.iter().enumerate() {
        println!("  {}. {}", i + 1, url_config.url);
        if let Some(status) = url_config.expected_status {
            println!("     期望状态码: {}", status);
        }
        if let Some(keyword) = &url_config.expected_keyword {
            println!("     期望关键词: {}", keyword);
        }
    }
    println!("{}", "=".repeat(60));

    let client = Client::builder().build()?;

    let mut url_statuses: Vec<UrlStatus> =
        (0..config.urls.len()).map(|_| UrlStatus::new()).collect();

    loop {
        let mut failed_results = Vec::new();
        let mut recovered_results = Vec::new();

        // 检查所有 URL
        for (i, url_config) in config.urls.iter().enumerate() {
            let result = check_url(&client, url_config);
            let status = &mut url_statuses[i];

            if result.success {
                println!(
                    "[{}] ✅ {} - 状态码: {:?} ({}ms)",
                    Local::now().format("%Y-%m-%d %H:%M:%S"),
                    result.url,
                    result.status_code,
                    result.response_time_ms
                );

                // 如果之前是失败状态，现在恢复了
                if !status.last_success {
                    recovered_results.push(result);
                }

                status.consecutive_failures = 0;
                status.last_success = true;
            } else {
                println!(
                    "[{}] ❌ {} - 失败: {:?} ({}ms)",
                    Local::now().format("%Y-%m-%d %H:%M:%S"),
                    result.url,
                    result
                        .error_message
                        .as_ref()
                        .unwrap_or(&"未知错误".to_string()),
                    result.response_time_ms
                );

                status.consecutive_failures += 1;
                status.last_success = false;

                // 达到失败阈值才记录需要通知
                if status.consecutive_failures >= config.failure_threshold {
                    // 避免频繁通知（每30分钟最多一次）
                    let should_notify = match status.last_notification_time {
                        Some(last_time) => {
                            let elapsed = Local::now() - last_time;
                            elapsed.num_minutes() >= 30
                        }
                        None => true,
                    };

                    if should_notify {
                        failed_results.push(result);
                        status.last_notification_time = Some(Local::now());
                    }
                }
            }
        }

        // 发送失败通知
        if !failed_results.is_empty() {
            if let Err(e) = send_feishu_notification(&config, &failed_results, false) {
                eprintln!("发送飞书通知失败: {}", e);
            }
        }

        // 发送恢复通知
        if config.recovery_notification && !recovered_results.is_empty() {
            if let Err(e) = send_feishu_notification(&config, &recovered_results, true) {
                eprintln!("发送恢复通知失败: {}", e);
            }
        }

        println!("--- 等待 {} 秒后下次检查 ---", config.check_interval_secs);
        thread::sleep(Duration::from_secs(config.check_interval_secs));
    }
}
