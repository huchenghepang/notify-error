// 为 feishu_client 模块编写测试
// 这是一个集成测试，直接测试公开的 API

use url_monitor::feishu_client::{
    generate_signature, FeishuConfig, FeishuContentBlock, FeishuMessageBuilder,
};

#[test]
fn test_feishu_message_builder_new() {
    let builder = FeishuMessageBuilder::new("Test Title");
    assert_eq!(builder.title, "Test Title");
}

#[test]
fn test_feishu_message_builder_add_text() {
    let builder = FeishuMessageBuilder::new("Title").add_text("Hello World");

    // 测试标题是否正确设置
    assert_eq!(builder.title, "Title");
}

#[test]
fn test_feishu_message_builder_add_url_info() {
    let builder = FeishuMessageBuilder::new("Title").add_url_info(
        "https://example.com",
        Some(200),
        Some("OK"),
        100,
    );

    assert_eq!(builder.title, "Title");
}

#[test]
fn test_feishu_message_builder_add_custom_message() {
    let builder = FeishuMessageBuilder::new("Title").add_custom_message("Custom message");

    assert_eq!(builder.title, "Title");
}

#[test]
fn test_feishu_message_builder_add_at_user() {
    let builder = FeishuMessageBuilder::new("Title").add_at_user("user123");

    assert_eq!(builder.title, "Title");
}

#[test]
fn test_feishu_message_builder_add_at_users() {
    let users = vec!["user1".to_string(), "user2".to_string()];
    let builder = FeishuMessageBuilder::new("Title").add_at_users(users);

    assert_eq!(builder.title, "Title");
}

#[test]
fn test_feishu_config_new() {
    let config = FeishuConfig::new("https://example.com/webhook".to_string(), None);
    assert_eq!(config.webhook_url, "https://example.com/webhook");
    assert!(config.secret.is_none());

    let config_with_secret = FeishuConfig::new(
        "https://example.com/webhook".to_string(),
        Some("secret123".to_string()),
    );
    assert_eq!(
        config_with_secret.webhook_url,
        "https://example.com/webhook"
    );
    assert_eq!(config_with_secret.secret, Some("secret123".to_string()));
}

#[test]
fn test_feishu_content_block_enum() {
    // 测试枚举类型的创建
    let text_block = FeishuContentBlock::Text("Hello".to_string());
    let url_block = FeishuContentBlock::UrlInfo {
        url: "https://example.com".to_string(),
        status_code: Some(200),
        error_message: Some("OK".to_string()),
        response_time_ms: 100,
    };
    let custom_block = FeishuContentBlock::CustomMessage("Custom".to_string());
    let section_block = FeishuContentBlock::Section(vec![text_block]);

    match url_block {
        FeishuContentBlock::UrlInfo {
            url, status_code, ..
        } => {
            assert_eq!(url, "https://example.com");
            assert_eq!(status_code, Some(200));
        }
        _ => panic!("Expected UrlInfo variant"),
    }

    match custom_block {
        FeishuContentBlock::CustomMessage(msg) => {
            assert_eq!(msg, "Custom");
        }
        _ => panic!("Expected CustomMessage variant"),
    }

    match section_block {
        FeishuContentBlock::Section(_) => {
            // 成功匹配
        }
        _ => panic!("Expected Section variant"),
    }
}

#[test]
fn test_feishu_message_builder_fluent_interface() {
    // 测试流畅接口模式
    let builder = FeishuMessageBuilder::new("Title")
        .add_text("First line")
        .add_text("Second line")
        .add_custom_message("Custom message")
        .add_at_user("user123");

    assert_eq!(builder.title, "Title");
}

// 异步测试
#[tokio::test]
async fn test_generate_signature() {
    // 测试签名生成函数
    let (timestamp, signature) = generate_signature("test_secret");

    // 验证时间戳是数字字符串
    assert!(timestamp.parse::<i64>().is_ok());

    // 验证签名不是空的
    assert!(!signature.is_empty());
}
