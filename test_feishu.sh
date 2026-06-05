#!/bin/bash
# 飞书 webhook 测试脚本

echo "🔍 开始飞书机器人真实环境测试..."
echo "📁 检查 .env.test 文件..."

if [ ! -f ".env.test" ]; then
    echo "❌ .env.test 文件不存在，请创建该文件并添加飞书 webhook 配置"
    echo "📝 示例内容："
    echo "FEISHU_WEBHOOK_URL=your_webhook_url_here"
    echo "FEISHU_BOT_SECRET=your_bot_secret_here"
    exit 1
fi

echo "✅ .env.test 文件存在"

# 加载环境变量并运行测试
echo "🚀 运行飞书 webhook 集成测试..."
RUST_LOG=info cargo run --example feishu_real_test

if [ $? -eq 0 ]; then
    echo ""
    echo "🎉 测试完成！请检查您的飞书群组是否收到了测试消息。"
    echo "✨ 如果收到了消息，说明您的飞书 webhook 集成已成功配置！"
else
    echo ""
    echo "💥 测试过程中发生错误，请检查错误信息"
fi