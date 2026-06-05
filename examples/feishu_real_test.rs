use std::env;
use url_monitor::feishu_client::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从 .env.test 加载环境变量
    match dotenv::from_filename(".env.test") {
        Ok(_) => println!("✅ 成功加载 .env.test 文件"),
        Err(e) => eprintln!("⚠️  未能加载 .env.test 文件: {}", e),
    }

    // 获取环境变量
    let webhook_url =
        env::var("FEISHU_WEBHOOK_URL").expect("FEISHU_WEBHOOK_URL 必须在 .env.test 中设置");
    let secret = env::var("FEISHU_BOT_SECRET").ok();

    println!("准备发送消息到飞书机器人...");
    println!("Webhook URL 长度: {} 字符", webhook_url.len());

    // 创建飞书配置
    let config = FeishuConfig::new(webhook_url, secret);

    // 创建一个 reqwest 客户端
    let client = reqwest::Client::new();

    // 测试签名生成功能
    if let Some(ref sec) = config.secret {
        println!("正在测试签名生成功能...");
        let (timestamp, signature) = generate_signature(sec);
        println!(
            "✅ 签名生成成功 - 时间戳: {}, 签名长度: {} 字符",
            timestamp,
            signature.len()
        );
    }

    // 发送测试消息
    println!("\n正在发送测试消息...");
    let result = send_simple_feishu_message(
        &client,
        &config,
        "🚀 飞书机器人真实测试",
        "这是一条通过真实飞书 webhook 发送的测试消息！\n\n测试内容:\n• 时间: 2026-06-03\n• 环境: 真实飞书机器人\n• 功能: 消息推送验证\n\n如果收到此消息，说明集成成功！",
        vec![], // 暂时不 @ 任何人
    ).await;

    match result {
        Ok(_) => {
            println!("🎉 非常棒！消息已成功发送到您的飞书群组！");
            println!("✅ 真实飞书 webhook 测试通过！");
        }
        Err(e) => {
            eprintln!("❌ 消息发送失败: {}", e);
            // 打印一些调试信息
            println!("\n调试信息:");
            println!("- Webhook URL: {}...", &config.webhook_url[..40]);
            println!("- 是否有密钥: {}", config.secret.is_some());
            if config.secret.is_some() {
                println!("- 密钥长度: {} 字符", config.secret.as_ref().unwrap().len());
            }
        }
    }

    // 测试表格数据发送功能（使用新版本的函数）
    println!("\n正在测试表格数据发送功能...");
    let headers = vec![
        "服务名称".to_string(),
        "状态".to_string(),
        "可用率(%)".to_string(),
        "响应时间(ms)".to_string(),
    ];
    let rows = vec![
        vec![
            "API服务".to_string(),
            "运行中".to_string(),
            "99.9".to_string(),
            "120".to_string(),
        ],
        vec![
            "数据库".to_string(),
            "运行中".to_string(),
            "99.8".to_string(),
            "80".to_string(),
        ],
        vec![
            "缓存服务".to_string(),
            "运行中".to_string(),
            "99.95".to_string(),
            "20".to_string(),
        ],
        vec![
            "消息队列".to_string(),
            "运行中".to_string(),
            "99.7".to_string(),
            "50".to_string(),
        ],
        vec![
            "前端服务".to_string(),
            "维护中".to_string(),
            "0".to_string(),
            "N/A".to_string(),
        ],
    ];

    let table_result =
        send_table_data(&client, &config, "📊 服务状态监控报表", headers, rows).await;
    match table_result {
        Ok(_) => println!("✅ 表格数据测试消息发送成功！"),
        Err(e) => println!("❌ 表格数据测试消息发送失败: {}", e),
    }

    // 测试三列表格的便利函数（保持向后兼容）
    println!("\n正在测试三列表格便利函数...");
    let triple_col_result = send_triple_column_table(
        &client,
        &config,
        "📈 三列表格测试",
        ("项目", "状态", "完成度%"),
        vec![
            (
                "用户认证模块".to_string(),
                "已完成".to_string(),
                "100%".to_string(),
            ),
            (
                "API网关".to_string(),
                "开发中".to_string(),
                "75%".to_string(),
            ),
            (
                "数据分析".to_string(),
                "设计中".to_string(),
                "25%".to_string(),
            ),
        ],
    )
    .await;
    match triple_col_result {
        Ok(_) => println!("✅ 三列表格测试消息发送成功！"),
        Err(e) => println!("❌ 三列表格测试消息发送失败: {}", e),
    }

    println!("\n真实环境测试完成！");
    Ok(())
}
