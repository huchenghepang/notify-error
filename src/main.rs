use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Local;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use url_monitor::feishu_client::{self, CheckResult, FeishuConfig};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};
use tokio::time::sleep;

// URL 监控配置
#[derive(Debug, Clone, Deserialize)]
struct UrlMonitorConfig {
    url: String,
    expected_status: Option<u16>,
    expected_keyword: Option<String>,
    timeout_secs: u64,
    method: String,
    headers: HashMap<String, String>,
    request_body: Option<String>,
    custom_alert_message: Option<String>,
}

// 全局配置
struct Config {
    check_interval_secs: u64,
    urls: Vec<UrlMonitorConfig>,
    feishu_webhook_url: String,
    feishu_secret: Option<String>,
    feishu_user_ids: Vec<String>,
    failure_threshold: u32,
    recovery_notification: bool,
    parallel_checks: bool,
    max_parallel_checks: usize,
}

impl Config {
    fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

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

        let parallel_checks = std::env::var("PARALLEL_CHECKS")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap_or(true);

        let max_parallel_checks = std::env::var("MAX_PARALLEL_CHECKS")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .unwrap_or(5);

        let urls = Self::parse_urls_config()?;

        Ok(Config {
            check_interval_secs,
            urls,
            feishu_webhook_url,
            feishu_secret,
            feishu_user_ids,
            failure_threshold,
            recovery_notification,
            parallel_checks,
            max_parallel_checks,
        })
    }

    fn parse_urls_config() -> Result<Vec<UrlMonitorConfig>> {
        let config_content = match fs::read_to_string("config.json") {
            Ok(content) => content,
            Err(_) => {
                Self::create_default_config()?;
                fs::read_to_string("config.json")?
            }
        };

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
            let request_body = config["request_body"].as_str().map(String::from);

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
                request_body,
                custom_alert_message,
            });
        }

        if urls.is_empty() {
            return Err(anyhow!("至少需要配置一个 URL"));
        }

        Ok(urls)
    }

    fn create_default_config() -> Result<()> {
        let default_config = json!({
            "urls": [
                {
                    "url": "https://www.google.com",
                    "expected_status": 200,
                    "expected_keyword": null,
                    "timeout_secs": 10,
                    "method": "GET",
                    "headers": {},
                    "request_body": null,
                    "custom_alert_message": null
                },
                {
                    "url": "https://www.github.com",
                    "expected_status": 200,
                    "expected_keyword": "GitHub",
                    "timeout_secs": 10,
                    "method": "GET",
                    "headers": {},
                    "request_body": null,
                    "custom_alert_message": null
                }
            ]
        });

        fs::write("config.json", serde_json::to_string_pretty(&default_config)?)?;
        println!("✅ 已创建默认 config.json 文件，请根据需要修改配置");
        Ok(())
    }
}

// 异步检查 URL 可用性
async fn check_url_async(client: &Client, config: &UrlMonitorConfig) -> CheckResult {
    let start = Instant::now();

    let mut request_builder = match config.method.to_uppercase().as_str() {
        "GET" => client.get(&config.url),
        "POST" => client.post(&config.url),
        "PUT" => client.put(&config.url),
        "DELETE" => client.delete(&config.url),
        "PATCH" => client.patch(&config.url),
        "HEAD" => client.head(&config.url),
        _ => {
            return CheckResult {
                url: config.url.clone(),
                success: false,
                status_code: None,
                error_message: Some(format!("不支持的 HTTP 方法: {}", config.method)),
                response_time_ms: 0,
            }
        }
    };

    for (key, value) in &config.headers {
        request_builder = request_builder.header(key, value);
    }

    if let Some(body) = &config.request_body {
        request_builder = request_builder.body(body.clone());
    }

    let response = request_builder
        .timeout(Duration::from_secs(config.timeout_secs))
        .send()
        .await;

    let response_time_ms = start.elapsed().as_millis() as u64;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let status_code = status.as_u16();

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

            if let Some(keyword) = &config.expected_keyword {
                match resp.text().await {
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
        Err(e) => {
            let error_msg = if e.is_connect() {
                format!("连接失败: {}", e)
            } else if e.is_timeout() {
                format!("请求超时 (超过{}秒)", config.timeout_secs)
            } else if e.is_request() {
                format!("请求错误: {}", e)
            } else if e.is_redirect() {
                format!("重定向错误: {}", e)
            } else if e.is_body() {
                format!("响应体读取错误: {}", e)
            } else if e.is_decode() {
                format!("解码错误: {}", e)
            } else {
                format!("网络错误: {}", e)
            };

            CheckResult {
                url: config.url.clone(),
                success: false,
                status_code: None,
                error_message: Some(error_msg),
                response_time_ms,
            }
        }
    }
}

// 生成飞书 Webhook 签名
fn generate_signature(secret: &str) -> (String, String) {
    let timestamp = Local::now().timestamp();
    let ts = timestamp.to_string();

    let key_string = format!("{}\n{}", ts, secret);

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key_string.as_bytes()).expect("HMAC key is valid");
    mac.update(b"");
    let result = mac.finalize();
    let sign = general_purpose::STANDARD.encode(result.into_bytes());

    (ts, sign)
}

async fn send_feishu_notification_with_client(
    config: &Config,
    results: &[CheckResult],
    is_recovery: bool,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| anyhow!("创建客户端失败: {}", e))?;

    let feishu_config = FeishuConfig::new(
        config.feishu_webhook_url.clone(),
        config.feishu_secret.clone(),
    );

    let url_configs: Vec<feishu_client::UrlMonitorConfig> = config
        .urls
        .iter()
        .map(|url_info| feishu_client::UrlMonitorConfig {
            url: url_info.url.clone(),
            expected_status: url_info.expected_status,
            expected_keyword: url_info.expected_keyword.clone(),
            timeout_secs: url_info.timeout_secs,
            method: url_info.method.clone(),
            request_body: url_info.request_body.clone(),
            custom_alert_message: url_info.custom_alert_message.clone(),
            headers: url_info.headers.clone(),
        })
        .collect();

    feishu_client::send_service_notification(
        &client,
        &feishu_config,
        results,
        is_recovery,
        &url_configs,
    )
    .await
}

// URL 状态追踪器
#[derive(Debug)]
struct UrlStatus {
    consecutive_failures: u32,
    last_success: bool,
    last_notification_time: Option<chrono::DateTime<Local>>,
    has_recovered_since_last_failure: bool,
    // 新增：记录当前是否需要发送通知（用于重试）
    pending_notification: bool,
}

impl UrlStatus {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_success: true,
            last_notification_time: None,
            has_recovered_since_last_failure: false,
            pending_notification: false,
        }
    }

    fn update(&mut self, result: &CheckResult, config: &Config) -> (bool, bool) {
        let now = Local::now();
        let mut should_notify_failure = false;
        let mut should_notify_recovery = false;

        if result.success {
            // 服务恢复正常
            if !self.last_success {
                should_notify_recovery = true;
                self.last_notification_time = None;
                self.has_recovered_since_last_failure = true;
                self.pending_notification = false; // 恢复时清除待发送标记
            }
            self.consecutive_failures = 0;
            self.last_success = true;
        } else {
            // 服务检查失败
            self.consecutive_failures += 1;
            
            if self.last_success {
                self.has_recovered_since_last_failure = false;
            }
            
            self.last_success = false;

            // 判断是否需要发送失败通知（包括待重试的情况）
            let should_send = if self.consecutive_failures >= config.failure_threshold {
                if self.pending_notification {
                    // 有待发送的通知，继续尝试
                    true
                } else {
                    match self.last_notification_time {
                        Some(last_time) => {
                            let elapsed = now - last_time;
                            self.has_recovered_since_last_failure || elapsed.num_minutes() >= 30
                        }
                        None => true,
                    }
                }
            } else {
                false
            };

            if should_send {
                should_notify_failure = true;
                // 注意：这里不立即更新 last_notification_time，等待发送成功后再更新
                self.pending_notification = true;
            }
        }

        (should_notify_failure, should_notify_recovery)
    }

    // 标记通知已成功发送
    fn mark_notification_sent(&mut self) {
        self.pending_notification = false;
        self.last_notification_time = Some(Local::now());
        self.has_recovered_since_last_failure = false;
    }
}

// 打印检查结果
fn print_check_result(result: &CheckResult) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    if result.success {
        println!(
            "[{}] ✅ {} - 状态码: {:?} ({}ms)",
            timestamp, result.url, result.status_code, result.response_time_ms
        );
    } else {
        println!(
            "[{}] ❌ {} - 失败: {} ({}ms)",
            timestamp,
            result.url,
            result.error_message.as_ref().unwrap_or(&"未知错误".to_string()),
            result.response_time_ms
        );
    }
}

// 并行检查所有 URL（异步版本）
async fn check_urls_parallel(
    client: &Client,
    config: &Config,
    url_statuses: &mut [UrlStatus],
) -> (Vec<CheckResult>, Vec<CheckResult>) {
    let futures: Vec<_> = config
        .urls
        .iter()
        .map(|url_config| check_url_async(client, url_config))
        .collect();
    
    let results = futures::future::join_all(futures).await;

    let mut failed_results = Vec::new();
    let mut recovered_results = Vec::new();

    for result in &results {
        print_check_result(result);
    }

    for (i, result) in results.iter().enumerate() {
        let (should_notify_failure, should_notify_recovery) =
            url_statuses[i].update(result, config);

        if should_notify_failure {
            failed_results.push(result.clone());
        }
        if should_notify_recovery && config.recovery_notification {
            recovered_results.push(result.clone());
        }
    }

    (failed_results, recovered_results)
}

// 串行检查所有 URL（异步版本）
async fn check_urls_serial(
    client: &Client,
    config: &Config,
    url_statuses: &mut [UrlStatus],
) -> (Vec<CheckResult>, Vec<CheckResult>) {
    let mut failed_results = Vec::new();
    let mut recovered_results = Vec::new();
    let mut results = Vec::new();

    for url_config in &config.urls {
        let result = check_url_async(client, url_config).await;
        print_check_result(&result);
        results.push(result);
    }

    for (i, result) in results.iter().enumerate() {
        let (should_notify_failure, should_notify_recovery) =
            url_statuses[i].update(result, config);

        if should_notify_failure {
            failed_results.push(result.clone());
        }
        if should_notify_recovery && config.recovery_notification {
            recovered_results.push(result.clone());
        }
    }

    (failed_results, recovered_results)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 启动 URL 监控服务");
    println!("{}", "=".repeat(60));

    let config = Config::from_env()?;
    println!("📋 配置信息:");
    println!("  - 飞书 Webhook URL: {}", config.feishu_webhook_url);
    println!("  - 检查间隔: {} 秒", config.check_interval_secs);
    println!("  - 失败阈值: {} 次", config.failure_threshold);
    println!("  - 恢复通知: {}", config.recovery_notification);
    println!("  - 并行检查: {}", config.parallel_checks);
    if config.parallel_checks {
        println!("  - 最大并行数: {}", config.max_parallel_checks);
    }
    println!("  - 监控 URL 数量: {}", config.urls.len());
    println!(
        "  - 飞书签名密钥: {}",
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
        if let Some(body) = &url_config.request_body {
            println!("     请求体: {}", body);
        }
    }
    println!("{}", "=".repeat(60));

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| anyhow!("创建 HTTP 客户端失败: {}", e))?;

    let mut url_statuses: Vec<UrlStatus> = (0..config.urls.len()).map(|_| UrlStatus::new()).collect();

    loop {
        let (failed_results, recovered_results) = if config.parallel_checks {
            check_urls_parallel(&client, &config, &mut url_statuses).await
        } else {
            check_urls_serial(&client, &config, &mut url_statuses).await
        };

        // 发送失败通知（带重试逻辑）
        if !failed_results.is_empty() {
            match send_feishu_notification_with_client(&config, &failed_results, false).await {
                Ok(_) => {
                    println!("✅ 失败通知发送成功");
                    // 发送成功后，标记通知已发送
                    for status in &mut url_statuses {
                        if status.pending_notification {
                            status.mark_notification_sent();
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ 发送飞书通知失败: {}", e);
                    // 发送失败时不更新状态，下次循环会继续尝试
                    println!("⚠️ 将在下次检查时重试发送通知");
                }
            }
        }

        // 发送恢复通知
        if config.recovery_notification && !recovered_results.is_empty() {
            match send_feishu_notification_with_client(&config, &recovered_results, true).await {
                Ok(_) => {
                    println!("✅ 恢复通知发送成功");
                }
                Err(e) => {
                    eprintln!("❌ 发送恢复通知失败: {}", e);
                }
            }
        }

        println!("--- 等待 {} 秒后下次检查 ---", config.check_interval_secs);
        sleep(Duration::from_secs(config.check_interval_secs)).await;
    }
}