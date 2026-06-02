use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Local;
use hmac::{Hmac, Mac};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};
use rayon::prelude::*;

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

// 检查结果
#[derive(Debug, Clone)]
struct CheckResult {
    url: String,
    success: bool,
    status_code: Option<u16>,
    error_message: Option<String>,
    response_time_ms: u64,
}

// 检查 URL 可用性
fn check_url(client: &Client, config: &UrlMonitorConfig) -> CheckResult {
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
        .send();

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

    // 使用 timestamp + "\n" + secret 作为 HMAC 密钥
    let key_string = format!("{}\n{}", ts, secret);

    type HmacSha256 = Hmac<Sha256>;
    // 使用拼接后的字符串作为 HMAC 密钥
    let mut mac = HmacSha256::new_from_slice(key_string.as_bytes()).expect("HMAC key is valid");
    // 对空字符串进行加密（根据飞书官方文档）
    mac.update(b"");
    let result = mac.finalize();
    // 使用 Base64 编码（根据飞书官方文档）
    let sign = general_purpose::STANDARD.encode(result.into_bytes());

    (ts, sign)
}

// 发送飞书通知
fn send_feishu_notification_with_client(
    client: &Client,
    config: &Config,
    results: &[CheckResult],
    is_recovery: bool,
) -> Result<()> {
    let (title, content_text) = if is_recovery {
        ("✅ 服务恢复通知", "以下服务已恢复正常")
    } else {
        ("🚨 服务异常告警", "以下服务出现异常")
    };

    let mut details = String::new();
    for result in results {
        if let Some(url_config) = config.urls.iter().find(|c| c.url == result.url) {
            if let Some(ref custom_msg) = url_config.custom_alert_message {
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

    let mut request_builder = client.post(&config.feishu_webhook_url);

    if let Some(ref secret) = config.feishu_secret {
        let (timestamp, signature) = generate_signature(secret);
        request_builder = request_builder
            .header("Timestamp", timestamp)
            .header("Sign", signature);
    }

    // 如果设置了密钥，需要在请求体中添加timestamp和sign
    let final_content = if let Some(ref secret) = config.feishu_secret {
        let (timestamp, signature) = generate_signature(secret);
        json!({
            "timestamp": timestamp,
            "sign": signature,
            "msg_type": content["msg_type"],
            "content": content["content"]
        })
    } else {
        content
    };

    let response = request_builder
        .json(&final_content)
        .send()?;

    let status = response.status();
    let response_text = response.text().unwrap_or_default();

    // 解析响应以检查飞书API的错误码
    let success = if let Ok(json_response) = serde_json::from_str::<serde_json::Value>(&response_text) {
        // 飞书API通常用code字段表示结果，0表示成功
        if let Some(code) = json_response.get("code") {
            code.as_i64() == Some(0)  // 飞书API中code为0表示成功
        } else {
            // 如果没有code字段，按HTTP状态判断
            status.is_success()
        }
    } else {
        // 如果不能解析JSON，按HTTP状态判断
        status.is_success()
    };

    if success {
        println!(
            "[{}] 📤 飞书通知发送成功",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        Ok(())
    } else {
        // 解析飞书API的错误响应
        let error_detail = if let Ok(json_response) = serde_json::from_str::<serde_json::Value>(&response_text) {
            if let Some(code) = json_response.get("code") {
                if let Some(msg) = json_response.get("msg") {
                    format!("飞书API错误 - 代码: {}, 消息: {}", code, msg)
                } else {
                    format!("飞书API错误 - 代码: {}, 响应: {}", code, response_text)
                }
            } else {
                format!("HTTP {}: {}", status, response_text)
            }
        } else {
            format!("HTTP {}: {}", status, response_text)
        };
        
        Err(anyhow!("飞书通知发送失败: {}", error_detail))
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

    fn update(&mut self, result: &CheckResult, config: &Config) -> (bool, bool) {
        let now = Local::now();
        let mut should_notify_failure = false;
        let mut should_notify_recovery = false;

        if result.success {
            if !self.last_success {
                should_notify_recovery = true;
            }
            self.consecutive_failures = 0;
            self.last_success = true;
        } else {
            self.consecutive_failures += 1;
            self.last_success = false;

            if self.consecutive_failures >= config.failure_threshold {
                let should_notify = match self.last_notification_time {
                    Some(last_time) => {
                        let elapsed = now - last_time;
                        elapsed.num_minutes() >= 30
                    }
                    None => true,
                };

                if should_notify {
                    should_notify_failure = true;
                    self.last_notification_time = Some(now);
                }
            }
        }

        (should_notify_failure, should_notify_recovery)
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

// 并行检查所有 URL（修复所有权问题）
fn check_urls_parallel(
    client: &Client,
    config: &Config,
    url_statuses: &mut [UrlStatus],
) -> (Vec<CheckResult>, Vec<CheckResult>) {
    // 使用 rayon 并行执行检查
    let results: Vec<CheckResult> = config
        .urls
        .par_iter()
        .map(|url_config| check_url(client, url_config))
        .collect();

    let mut failed_results = Vec::new();
    let mut recovered_results = Vec::new();

    // 先打印所有结果
    for result in &results {
        print_check_result(result);
    }

    // 然后更新状态并收集需要通知的结果
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

// 串行检查所有 URL（修复所有权问题）
fn check_urls_serial(
    client: &Client,
    config: &Config,
    url_statuses: &mut [UrlStatus],
) -> (Vec<CheckResult>, Vec<CheckResult>) {
    let mut failed_results = Vec::new();
    let mut recovered_results = Vec::new();
    let mut results = Vec::new();

    // 先执行所有检查并收集结果
    for url_config in &config.urls {
        let result = check_url(client, url_config);
        print_check_result(&result);
        results.push(result);
    }

    // 然后更新状态并收集需要通知的结果
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

fn main() -> Result<()> {
    println!("🚀 启动 URL 监控服务");
    println!("{}", "=".repeat(60));

    let config = Config::from_env()?;
    println!("📋 配置信息:");
    println!("飞书机器人配置:");
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
        if let Some(body) = &url_config.request_body {
            println!("     请求体: {}", body);
        }
    }
    println!("{}", "=".repeat(60));

    // 创建可复用的 HTTP 客户端
    let shared_client = Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| anyhow!("创建 HTTP 客户端失败: {}", e))?;

    // 创建通知专用客户端
    let notification_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("创建通知客户端失败: {}", e))?;

    let mut url_statuses: Vec<UrlStatus> = (0..config.urls.len()).map(|_| UrlStatus::new()).collect();

    loop {
        let (failed_results, recovered_results) = if config.parallel_checks {
            check_urls_parallel(&shared_client, &config, &mut url_statuses)
        } else {
            check_urls_serial(&shared_client, &config, &mut url_statuses)
        };

        // 发送失败通知
        if !failed_results.is_empty() {
            if let Err(e) = send_feishu_notification_with_client(
                &notification_client,
                &config,
                &failed_results,
                false,
            ) {
                eprintln!("❌ 发送飞书通知失败: {}", e);
            }
        }

        // 发送恢复通知
        if config.recovery_notification && !recovered_results.is_empty() {
            if let Err(e) = send_feishu_notification_with_client(
                &notification_client,
                &config,
                &recovered_results,
                true,
            ) {
                eprintln!("❌ 发送恢复通知失败: {}", e);
            }
        }

        println!("--- 等待 {} 秒后下次检查 ---", config.check_interval_secs);
        thread::sleep(Duration::from_secs(config.check_interval_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_url_success() {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let config = UrlMonitorConfig {
            url: "https://httpbin.org/status/200".to_string(),
            expected_status: Some(200),
            expected_keyword: None,
            timeout_secs: 5,
            method: "GET".to_string(),
            headers: HashMap::new(),
            request_body: None,
            custom_alert_message: None,
        };

        let result = check_url(&client, &config);
        assert!(result.success);
        assert_eq!(result.status_code, Some(200));
    }

    #[test]
    fn test_check_url_timeout() {
        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();

        let config = UrlMonitorConfig {
            url: "https://httpbin.org/delay/10".to_string(),
            expected_status: Some(200),
            expected_keyword: None,
            timeout_secs: 2,
            method: "GET".to_string(),
            headers: HashMap::new(),
            request_body: None,
            custom_alert_message: None,
        };

        let result = check_url(&client, &config);
        assert!(!result.success);
        assert!(result.error_message.unwrap().contains("超时"));
    }

    #[test]
    fn test_url_status_update() {
        let mut status = UrlStatus::new();
        let config = Config {
            check_interval_secs: 30,
            urls: vec![],
            feishu_webhook_url: "".to_string(),
            feishu_secret: None,
            feishu_user_ids: vec![],
            failure_threshold: 3,
            recovery_notification: true,
            parallel_checks: false,
            max_parallel_checks: 5,
        };

        let result_success = CheckResult {
            url: "https://test.com".to_string(),
            success: true,
            status_code: Some(200),
            error_message: None,
            response_time_ms: 100,
        };

        let (_, recovery) = status.update(&result_success, &config);
        assert!(!recovery);
        assert_eq!(status.consecutive_failures, 0);
        assert!(status.last_success);

        let result_failure = CheckResult {
            url: "https://test.com".to_string(),
            success: false,
            status_code: None,
            error_message: Some("Error".to_string()),
            response_time_ms: 100,
        };

        for i in 1..=3 {
            let (notify, _) = status.update(&result_failure, &config);
            if i >= 3 {
                assert!(notify);
            } else {
                assert!(!notify);
            }
        }
    }
}