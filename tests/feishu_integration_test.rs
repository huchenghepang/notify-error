use url_monitor::feishu_client::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从 .env.test 加载环境变量
    dotenv::dotenv().ok();
    
    // 获取环境变量
    let webhook_url = std::env::var("FEISHU_WEBHOOK_URL")
        .expect("FEISHU_WEBHOOK_URL 必须设置");
    let secret = std::env::var("FEISHU_BOT_SECRET").ok();
    
    println!("使用 webhook URL: {}", &webhook_url[..30]); // 只显示URL的一部分以保护隐私
    
    // 创建飞书配置
    let config = FeishuConfig::new(webhook_url, secret);
    
    // 创建一个 reqwest 客户端
    let client = reqwest::Client::new();
    
    // 测试1: 发送简单消息
    println!("正在发送测试消息...");
    let result = send_simple_feishu_message(
        &client,
        &config,
        "🧪 飞书机器人测试",
        "这是一条来自 Rust 应用的测试消息！\n测试时间: 2026-06-03",
        vec![], // 暂时不 @ 任何人
    ).await;
    
    match result {
        Ok(_) => println!("✅ 测试消息发送成功！"),
        Err(e) => println!("❌ 测试消息发送失败: {}", e),
    }
    
    // 测试2: 发送带有URL信息的消息
    println!("\n正在发送URL信息测试消息...");
    let message_builder = FeishuMessageBuilder::new("🌐 URL监控测试")
        .add_text("这是一个URL监控功能的测试")
        .add_url_info(
            "https://www.baidu.com",
            Some(200),
            None, // 没有错误
            150,  // 响应时间150ms
        )
        .add_url_info(
            "https://httpstat.us/500",
            Some(500),
            Some("服务器内部错误"),
            800, // 响应时间800ms
        )
        .add_text("测试完成！");
    
    let result = send_feishu_message(&client, &config, message_builder).await;
    
    match result {
        Ok(_) => println!("✅ URL信息测试消息发送成功！"),
        Err(e) => println!("❌ URL信息测试消息发送失败: {}", e),
    }
    
    // 测试3: 发送服务通知
    println!("\n正在发送服务通知测试消息...");
    
    // 创建测试数据
    let check_results = vec![
        CheckResult {
            url: "https://www.rust-lang.org".to_string(),
            status_code: Some(200),
            error_message: None,
            response_time_ms: 120,
            success: true,
        },
        CheckResult {
            url: "https://httpstat.us/404".to_string(),
            status_code: Some(404),
            error_message: Some("页面未找到".to_string()),
            response_time_ms: 300,
            success: false,
        }
    ];
    
    let url_configs = vec![
        UrlMonitorConfig {
            url: "https://www.rust-lang.org".to_string(),
            expected_status: Some(200),
            expected_keyword: Some("Rust".to_string()),
            timeout_secs: 10,
            method: "GET".to_string(),
            headers: std::collections::HashMap::new(),
            request_body: None,
            custom_alert_message: Some("✅ {url} 状态正常，耗时: {response_time}ms".to_string()),
        },
        UrlMonitorConfig {
            url: "https://httpstat.us/404".to_string(),
            expected_status: Some(200),
            expected_keyword: None,
            timeout_secs: 10,
            method: "GET".to_string(),
            headers: std::collections::HashMap::new(),
            request_body: None,
            custom_alert_message: Some("🚨 {url} 异常: {error_message}，耗时: {response_time}ms".to_string()),
        }
    ];
    
    let result = send_service_notification(
        &client,
        &config,
        &check_results,
        false, // 不是恢复通知，而是故障通知
        &url_configs,
    ).await;
    
    match result {
        Ok(_) => println!("✅ 服务通知测试消息发送成功！"),
        Err(e) => println!("❌ 服务通知测试消息发送失败: {}", e),
    }
    
    println!("\n所有测试完成！");
    Ok(())
}