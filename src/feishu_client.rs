use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine};
use chrono::Local;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

// 移除对 crate 根中类型的依赖，定义本地类型别名或重新定义所需类型

use std::collections::HashMap;

// 定义在 send_service_notification 函数中使用的类型
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub url: String,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
    pub response_time_ms: u64,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct UrlMonitorConfig {
    pub url: String,
    pub expected_status: Option<u16>,
    pub expected_keyword: Option<String>,
    pub timeout_secs: u64,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub request_body: Option<String>,
    pub custom_alert_message: Option<String>,
}

// ============ 通用数据结构 ============

/// 飞书消息构建器
pub struct FeishuMessageBuilder {
    pub title: String,
    pub content_blocks: Vec<FeishuContentBlock>,
    pub at_users: Vec<String>,
}

/// 内容块类型
pub enum FeishuContentBlock {
    Text(String),
    UrlInfo {
        url: String,
        status_code: Option<u16>,
        error_message: Option<String>,
        response_time_ms: u64,
    },
    CustomMessage(String),
    Section(Vec<FeishuContentBlock>),
}

impl FeishuMessageBuilder {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content_blocks: Vec::new(),
            at_users: Vec::new(),
        }
    }

    pub fn add_text(mut self, text: impl Into<String>) -> Self {
        self.content_blocks
            .push(FeishuContentBlock::Text(text.into()));
        self
    }

    pub fn add_url_info(
        mut self,
        url: &str,
        status_code: Option<u16>,
        error_message: Option<&str>,
        response_time_ms: u64,
    ) -> Self {
        self.content_blocks.push(FeishuContentBlock::UrlInfo {
            url: url.to_string(),
            status_code,
            error_message: error_message.map(|s| s.to_string()),
            response_time_ms,
        });
        self
    }

    pub fn add_custom_message(mut self, message: impl Into<String>) -> Self {
        self.content_blocks
            .push(FeishuContentBlock::CustomMessage(message.into()));
        self
    }

    pub fn add_section(mut self, blocks: Vec<FeishuContentBlock>) -> Self {
        self.content_blocks
            .push(FeishuContentBlock::Section(blocks));
        self
    }

    pub fn add_at_user(mut self, user_id: impl Into<String>) -> Self {
        self.at_users.push(user_id.into());
        self
    }

    pub fn add_at_users(mut self, user_ids: Vec<String>) -> Self {
        self.at_users.extend(user_ids);
        self
    }

    /// 构建消息内容
    fn build_content(&self) -> String {
        let mut content = String::new();

        for block in &self.content_blocks {
            match block {
                FeishuContentBlock::Text(text) => {
                    content.push_str(text);
                    content.push('\n');
                }
                FeishuContentBlock::UrlInfo {
                    url,
                    status_code,
                    error_message,
                    response_time_ms,
                } => {
                    content.push_str(&format!("\n**{}**", url));
                    if let Some(status) = status_code {
                        content.push_str(&format!("\n- 状态码: {}", status));
                    }
                    if let Some(err) = error_message {
                        content.push_str(&format!("\n- 错误: {}", err));
                    }
                    content.push_str(&format!("\n- 响应时间: {}ms", response_time_ms));
                    content.push('\n');
                }
                FeishuContentBlock::CustomMessage(msg) => {
                    content.push_str(msg);
                    content.push('\n');
                }
                FeishuContentBlock::Section(blocks) => {
                    for sub_block in blocks {
                        match sub_block {
                            FeishuContentBlock::Text(text) => {
                                content.push_str(text);
                            }
                            FeishuContentBlock::CustomMessage(msg) => {
                                content.push_str(msg);
                            }
                            _ => {
                                // 递归处理其他类型
                                if let FeishuContentBlock::Section(sub_blocks) = sub_block {
                                    for sb in sub_blocks {
                                        if let FeishuContentBlock::Text(t) = sb {
                                            content.push_str(t);
                                        } else if let FeishuContentBlock::CustomMessage(m) = sb {
                                            content.push_str(m);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    content.push('\n');
                }
            }
        }

        // 添加 @ 用户
        if !self.at_users.is_empty() {
            let ats: Vec<String> = self
                .at_users
                .iter()
                .map(|id| format!("<at id={}></at>", id))
                .collect();
            content.push_str(&format!("\n{}", ats.join(" ")));
        }

        content
    }

    /// 转换为飞书API格式
    fn to_json_value(&self) -> serde_json::Value {
        let content_text = self.build_content();

        json!({
            "msg_type": "post",
            "content": {
                "post": {
                    "zh_cn": {
                        "title": self.title,
                        "content": [
                            [
                                {
                                    "tag": "text",
                                    "text": content_text
                                }
                            ]
                        ]
                    }
                }
            }
        })
    }
}

// ============ 通用发送函数 ============

/// 飞书配置
pub struct FeishuConfig {
    pub webhook_url: String,
    pub secret: Option<String>,
}

impl FeishuConfig {
    pub fn new(webhook_url: String, secret: Option<String>) -> Self {
        Self {
            webhook_url,
            secret,
        }
    }
}

pub fn generate_signature(secret: &str) -> (String, String) {
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

/// 通用发送函数
pub async fn send_feishu_message(
    client: &reqwest::Client,
    config: &FeishuConfig,
    message_builder: FeishuMessageBuilder,
) -> Result<()> {
    let content = message_builder.to_json_value();

    let mut request_builder = client.post(&config.webhook_url);

    // 准备最终请求体
    let final_content = if let Some(ref secret) = config.secret {
        let (timestamp, signature) = generate_signature(secret);
        request_builder = request_builder
            .header("Timestamp", &timestamp)
            .header("Sign", &signature);

        json!({
            "timestamp": timestamp,
            "sign": signature,
            "msg_type": content["msg_type"],
            "content": content["content"]
        })
    } else {
        content
    };

    let response = request_builder.json(&final_content).send().await?;

    let status = response.status();
    let response_text = response.text().await.unwrap_or_default();

    // 检查响应
    let success =
        if let Ok(json_response) = serde_json::from_str::<serde_json::Value>(&response_text) {
            if let Some(code) = json_response.get("code") {
                code.as_i64() == Some(0)
            } else {
                status.is_success()
            }
        } else {
            status.is_success()
        };

    if success {
        println!(
            "[{}] 📤 飞书通知发送成功",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        Ok(())
    } else {
        let error_detail =
            if let Ok(json_response) = serde_json::from_str::<serde_json::Value>(&response_text) {
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

// ============ 便捷宏 ============

/// 快速发送简单文本消息
pub async fn send_simple_feishu_message(
    client: &reqwest::Client,
    config: &FeishuConfig,
    title: &str,
    message: &str,
    at_users: Vec<String>,
) -> Result<()> {
    let builder = FeishuMessageBuilder::new(title)
        .add_text(message)
        .add_at_users(at_users);

    send_feishu_message(client, config, builder).await
}

// ============ 使用示例 ============

// 修复后的 send_service_notification 函数
pub async fn send_service_notification(
    client: &reqwest::Client,
    config: &FeishuConfig,
    results: &[CheckResult],
    is_recovery: bool,
    url_configs: &[UrlMonitorConfig],
) -> Result<()> {
    let title = if is_recovery {
        "✅ 服务恢复通知"
    } else {
        "🚨 服务异常告警"
    };

    let mut builder = FeishuMessageBuilder::new(title)
        .add_text(format!(
            "⏰ 时间: {}",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        ))
        .add_text(if is_recovery {
            "以下服务已恢复正常"
        } else {
            "以下服务出现异常"
        });

    for result in results {
        if let Some(url_config) = url_configs.iter().find(|c| c.url == result.url) {
            if let Some(custom_msg) = &url_config.custom_alert_message {
                // 修复：正确替换所有占位符
                let rendered = custom_msg
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
                        result.error_message.as_deref().unwrap_or("无错误信息"),
                    )
                    .replace("{response_time}", &result.response_time_ms.to_string())
                    .replace(
                        "{timestamp}",
                        &Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    )
                    // 添加对成功状态的特殊处理
                    .replace("{status}", if result.success { "成功" } else { "失败" });
                
                builder = builder.add_custom_message(rendered);
            } else {
                // 使用默认格式
                if result.success {
                    builder = builder.add_custom_message(format!(
                        "✅ {} - 恢复成功 ({}ms)",
                        result.url, result.response_time_ms
                    ));
                } else {
                    builder = builder.add_url_info(
                        &result.url,
                        result.status_code,
                        result.error_message.as_deref(),
                        result.response_time_ms,
                    );
                }
            }
        } else {
            // 如果没有找到配置，使用默认格式
            if result.success {
                builder = builder.add_custom_message(format!(
                    "✅ {} - 恢复成功 ({}ms)",
                    result.url, result.response_time_ms
                ));
            } else {
                builder = builder.add_url_info(
                    &result.url,
                    result.status_code,
                    result.error_message.as_deref(),
                    result.response_time_ms,
                );
            }
        }
    }

    send_feishu_message(client, config, builder).await
}
// 示例2：发送自定义格式的消息
pub async fn send_custom_report(client: &reqwest::Client, config: &FeishuConfig) -> Result<()> {
    let builder = FeishuMessageBuilder::new("📊 每日报告")
        .add_text("今日统计数据：")
        .add_custom_message("总请求数：1500")
        .add_custom_message("成功率：99.5%")
        .add_text("详细报告请查看附件")
        .add_at_user("user_123"); // @具体用户

    send_feishu_message(client, config, builder).await
}

// 示例3：发送表格形式的数据（支持自定义表头和多列）
pub async fn send_table_data<T: AsRef<[String]>>(
    client: &reqwest::Client,
    config: &FeishuConfig,
    title: &str,
    headers: Vec<String>,
    rows: Vec<T>,
) -> Result<()> {
    let mut builder = FeishuMessageBuilder::new(title);

    // 构建表头行
    let header_row = format!("| {} |", headers.join(" | "));
    builder = builder.add_text(&header_row);

    // 构建分隔行
    let separator = vec!["------"; headers.len()].join(" | ");
    let separator_row = format!("| {} |", separator);
    builder = builder.add_text(&separator_row);

    // 构建数据行
    for row in rows {
        let cells = row.as_ref();
        let data_row = format!("| {} |", cells.join(" | "));
        builder = builder.add_text(&data_row);
    }

    send_feishu_message(client, config, builder).await
}

// 便利函数：发送三列表格（保持向后兼容）
pub async fn send_triple_column_table(
    client: &reqwest::Client,
    config: &FeishuConfig,
    title: &str,
    headers: (&str, &str, &str),
    data: Vec<(String, String, String)>,
) -> Result<()> {
    let headers_vec = vec![
        headers.0.to_string(),
        headers.1.to_string(),
        headers.2.to_string(),
    ];
    let rows: Vec<Vec<String>> = data
        .into_iter()
        .map(|(col1, col2, col3)| vec![col1, col2, col3])
        .collect();

    send_table_data(client, config, title, headers_vec, rows).await
}
