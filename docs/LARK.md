# Bee 飞书（Lark）集成（可选）

通过飞书开放平台事件订阅与 Bee Agent 对话。**需要公网 Webhook 回调域名**（本地开发可用 ngrok 等穿透）。

## 前置条件

1. **飞书开放平台**：前往 [飞书开放平台](https://open.feishu.cn/) 或 [Lark Suite](https://open.larksuite.com/)（国际版）注册开发者
2. **创建企业自建应用**：在控制台创建应用，获取 `App ID` 和 `App Secret`
3. **开通权限**：在应用权限管理中开通
   - `im:message` / 发送消息
   - `im:message.group_at_msg` 或 `im:message.group_at_msg:readonly`（**群聊 @ 机器人必开**）
   - `im:message.p2p_msg:readonly` / **获取用户发给机器人的单聊消息**（**单聊必开**，否则私聊无回复）

## 环境变量

| 变量 | 说明 | 必填 |
|------|------|------|
| `LARK_APP_ID` | 飞书应用 App ID | 是 |
| `LARK_APP_SECRET` | 飞书应用 App Secret | 是 |
| `LARK_BASE_URL` | API 基地址（默认 `https://open.feishu.cn`，国际版用 `https://open.larksuite.com`） | 否 |
| `LARK_PORT` | 服务监听端口（默认 `3001`） | 否 |
| `DEEPSEEK_API_KEY` 或 `OPENAI_API_KEY` | LLM API Key | 是 |

## 构建与运行

```bash
# 构建飞书服务
cargo build --bin bee-lark --features lark

# 设置环境变量
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export DEEPSEEK_API_KEY=sk-xxx

# 运行（本地需配合 ngrok 等公网穿透）
cargo run --bin bee-lark --features lark
```

服务默认监听 `http://0.0.0.0:3001`。

## Webhook 配置

### 1. 公网 URL

本地开发需使用 [ngrok](https://ngrok.com/) 等工具将本机端口暴露到公网：

```bash
ngrok http 3001
# 获得类似 https://abc123.ngrok.io 的 URL
```

### 2. 飞书开放平台配置

1. 进入应用 → **事件订阅** → 启用事件订阅
2. **请求地址**：`https://你的域名/webhook`（需公网可访问）
3. **订阅事件**：勾选 `接收消息 (im.message.receive_v1)`
4. 保存后，飞书会向该 URL 发送校验请求（`type: "url_verification"`），Bee 会自动返回 `challenge` 完成校验

### 3. 应用发布与使用

- **开发阶段**：在飞书管理后台将应用添加为可用应用，或创建测试群组并添加机器人
- **发送消息**：在群聊或私聊中 @ 机器人 或直接发消息，Bee 会调用 Agent 并回复

## 端点说明

| 端点 | 方法 | 说明 |
|------|------|------|
| `/webhook` | POST | 飞书事件回调（URL 校验 + 接收消息） |
| `/health` | GET | 健康检查 |

## 事件处理

- **URL 校验**：收到 `type: "url_verification"` 时，返回 `{"challenge": challenge}`
- **消息接收**：收到 `im.message.receive_v1` 时，解析消息文本，调用 Agent，通过飞书 API 发送回复

## 架构

```
飞书用户 → 飞书服务器 → POST /webhook → Bee Agent (ReAct) → 飞书 API → 用户
```

- 按 `chat_id` 维护独立对话上下文
- 支持工具调用（cat, ls, echo）
- 长回复自动分段发送（每段 ≤ 4000 字符）

## 故障排查

### @ 机器人无回复

1. **群聊 @ 权限**：必须在飞书开放平台 → 应用 → 权限管理 中开通 **「获取群组中所有消息」** 或 **「获取用户在群组中@机器人的消息」**（`im:message.group_at_msg:readonly`），否则群聊中 @ 机器人的消息不会被推送
2. **发布新版本**：修改权限后需在控制台「版本管理」中**发布新版本**，否则权限不生效
3. **3 秒超时**：飞书要求 Webhook 在 3 秒内返回 200。Bee 已做异步处理（先返回再后台处理），若仍有问题可检查网络延迟

### 单聊/私聊无回复

需开通 **「获取用户发给机器人的单聊消息」**（`im:message.p2p_msg:readonly`），否则飞书不会将单聊消息推送给 Webhook。权限管理 → 搜索「单聊」或 `p2p` → 开通后**发布新版本**。

### 日志排查：后端是否收到消息？

运行时可开启日志，观察 @ 机器人时控制台输出：

```bash
RUST_LOG=info cargo run --bin bee-lark --features lark
```

| 日志内容 | 含义 |
|----------|------|
| `Lark webhook received: type=url_verification` | URL 校验请求（正常） |
| `Lark webhook received: type=event_callback` | 收到事件回调 |
| `Lark webhook received: type=(none)` | 可能是**加密请求**：事件订阅若配置了 Encrypt Key，需在控制台移除或实现解密 |
| **没有任何 "Lark webhook received"** | 请求未到达后端：检查 ngrok 是否运行、Webhook URL 是否正确、飞书能否访问你的公网地址 |
| `event type X not im.message.receive_v1` | 收到其他事件类型，非消息（可忽略） |
| `message_type X not text` | 非文本消息（图片/文件等），当前仅支持文本 |
| `accepted message ... spawning` | 已接受消息，正在后台处理 |
| `reply sent for chat_id=` | 回复已发送 |
| `Lark background process error:` | 后台处理失败（Agent 或发送 API 报错） |

### 其他

4. **URL 校验失败**：确认 Webhook 地址可从公网访问，且返回正确的 `challenge`
5. **收不到消息**：确认已订阅 `im.message.receive_v1`，应用已添加到群组/可用
6. **发送失败**：检查 `LARK_APP_ID`、`LARK_APP_SECRET` 及权限配置
7. **Agent 无响应**：确认 `DEEPSEEK_API_KEY` 或 `OPENAI_API_KEY` 已设置
