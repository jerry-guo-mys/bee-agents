# Bee WhatsApp 集成（可选）

通过 WhatsApp Cloud API 与 Bee Agent 对话。**需要公网 Webhook 回调域名**（本地开发可用 ngrok 等穿透），若无法提供可暂时不启用此功能。

## 前置条件

1. **Meta 开发者账号**：前往 [Meta for Developers](https://developers.facebook.com/) 注册
2. **WhatsApp Business 应用**：在 Meta 开发者控制台创建应用并添加 WhatsApp 产品
3. **电话号码**：配置 WhatsApp Business 电话号码（测试可使用 Meta 提供的测试号码）

## 环境变量

| 变量 | 说明 | 必填 |
|------|------|------|
| `WHATSAPP_ACCESS_TOKEN` | Meta WhatsApp API 访问令牌 | 是 |
| `WHATSAPP_PHONE_NUMBER_ID` | 企业电话号码 ID | 是 |
| `WHATSAPP_VERIFY_TOKEN` | Webhook 验证令牌（自定义，用于 Meta 验证） | 否，默认 `bee` |
| `DEEPSEEK_API_KEY` 或 `OPENAI_API_KEY` | LLM API Key | 是 |

## 构建与运行

```bash
# 构建 WhatsApp 服务
cargo build --bin bee-whatsapp --features whatsapp

# 设置环境变量
export WHATSAPP_ACCESS_TOKEN=your_access_token
export WHATSAPP_PHONE_NUMBER_ID=your_phone_number_id
export DEEPSEEK_API_KEY=sk-xxx

# 运行（本地需配合 ngrok 等公网穿透）
cargo run --bin bee-whatsapp --features whatsapp
```

服务默认监听 `http://0.0.0.0:3000`。

## Webhook 配置

1. **公网 URL**：本地开发需使用 [ngrok](https://ngrok.com/) 等工具将本机端口暴露到公网
   ```bash
   ngrok http 3000
   # 获得类似 https://abc123.ngrok.io 的 URL
   ```

2. **Meta 开发者控制台**：
   - 进入 WhatsApp > 配置 > Webhook
   - **回调 URL**：`https://你的域名/webhook`
   - **验证令牌**：与 `WHATSAPP_VERIFY_TOKEN` 一致（默认 `bee`）
   - **订阅字段**：勾选 `messages`

3. **验证**：点击「验证并保存」后，Meta 会向 `/webhook` 发送 GET 请求，Bee 将返回 challenge 完成验证。

## 端点说明

| 端点 | 方法 | 说明 |
|------|------|------|
| `/webhook` | GET | Meta Webhook 验证（返回 challenge） |
| `/webhook` | POST | 接收 WhatsApp 消息，调用 Agent 后回复 |
| `/health` | GET | 健康检查 |

## 架构

```
WhatsApp 用户 → Meta 服务器 → POST /webhook → Bee Agent (ReAct) → WhatsApp API → 用户
```

- 每个 WhatsApp 用户（`from` 号码）拥有独立的对话上下文
- 支持工具调用（cat, ls, echo）
- 长回复自动分段发送（每段 ≤ 4000 字符）

## 故障排查

1. **验证失败**：确认 `WHATSAPP_VERIFY_TOKEN` 与 Meta 控制台设置一致
2. **收不到消息**：确认 Webhook 已订阅 `messages`，且 URL 可从公网访问
3. **发送失败**：检查 `WHATSAPP_ACCESS_TOKEN` 和 `WHATSAPP_PHONE_NUMBER_ID` 是否正确
4. **Agent 无响应**：确认 `DEEPSEEK_API_KEY` 或 `OPENAI_API_KEY` 已设置
