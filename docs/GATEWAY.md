# 轮毂式（Hub-and-Spoke）网关架构

## 概述

将 AI 助手系统拆分为 Hub（中枢）和 Spoke（端点）两层。

### Hub（轮毂/中枢）- 核心运行时

Hub 是整个系统的大脑，包含：

- **LLM 路由网关**：模型选择、负载均衡、fallback
- **记忆系统**：短期对话日志 + 长期文件索引
- **意图识别**：理解用户意图，路由到合适的能力
- **决策引擎**：ReAct 循环、规划、执行

### Spoke（辐条/端点）- 外围接入点

Spoke 分为两类：

#### 1. 通讯端点（Communication Spokes）
- Telegram、Slack、WhatsApp、Discord
- 终端命令行（TUI）
- Web 浏览器
- HTTP API

#### 2. 能力端点（Capability Spokes）
- Skills 技能（知识增强、模板、脚本）
- 本地工具（文件操作、Shell、代码编辑）
- API 插件（搜索、浏览器、外部服务）
- 自动化脚本（Python/Shell）

## 架构优势

- **彻底解耦**：通讯层、决策层、能力层分离
- **跨平台上下文连贯**：在任何平台发消息都能保持对话
- **后台持续运行**：支持异步任务和长时间处理
- **统一会话管理**：用户维度的会话，跨设备同步
- **意图驱动**：智能路由到合适的能力端点

## 运行网关

```bash
# 启动网关服务器
cargo run --bin bee-gateway --features gateway

# 自定义绑定地址
GATEWAY_BIND=0.0.0.0:9000 cargo run --bin bee-gateway --features gateway
```

## WebSocket 协议

### 消息格式

所有消息都是 JSON 格式：

```json
{
  "id": "uuid",
  "session_id": "session_xxx",
  "message": { ... },
  "timestamp": 1234567890
}
```

### 消息类型

#### 1. 认证 (Auth)

客户端连接后首先发送认证：

```json
{
  "message": {
    "type": "auth",
    "token": null,
    "client_info": {
      "client_id": "user_123",
      "platform": "web",
      "display_name": "张三"
    }
  }
}
```

响应：

```json
{
  "message": {
    "type": "auth_result",
    "success": true,
    "session_id": "session_xxx"
  }
}
```

#### 2. 发送消息 (UserMessage)

```json
{
  "session_id": "session_xxx",
  "message": {
    "type": "user_message",
    "content": "你好",
    "assistant_id": null,
    "model": null
  }
}
```

#### 3. 流式响应

响应开始：

```json
{"message": {"type": "response_start", "request_id": "req_xxx"}}
```

响应片段：

```json
{"message": {"type": "response_chunk", "request_id": "req_xxx", "content": "你好"}}
```

响应结束：

```json
{"message": {"type": "response_end", "request_id": "req_xxx", "full_content": "你好！有什么..."}}
```

#### 4. 工具调用

```json
{
  "message": {
    "type": "tool_call",
    "request_id": "req_xxx",
    "tool_name": "search",
    "arguments": {"query": "..."}
  }
}
```

```json
{
  "message": {
    "type": "tool_result",
    "request_id": "req_xxx",
    "tool_name": "search",
    "result": "...",
    "success": true
  }
}
```

#### 5. 心跳

```json
{"message": {"type": "ping", "timestamp": 1234567890}}
```

```json
{"message": {"type": "pong", "timestamp": 1234567890}}
```

#### 6. 取消请求

```json
{"message": {"type": "cancel", "request_id": "req_xxx"}}
```

#### 7. 获取历史

```json
{"message": {"type": "get_history", "limit": 10}}
```

## JavaScript 客户端示例

```javascript
const ws = new WebSocket('ws://localhost:9000');

ws.onopen = () => {
  // 认证
  ws.send(JSON.stringify({
    id: crypto.randomUUID(),
    message: {
      type: 'auth',
      token: null,
      client_info: {
        client_id: 'user_' + Date.now(),
        platform: 'web',
        display_name: 'Web User'
      }
    },
    timestamp: Date.now()
  }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  
  switch (msg.message.type) {
    case 'auth_result':
      console.log('Session ID:', msg.message.session_id);
      sessionId = msg.message.session_id;
      break;
      
    case 'response_chunk':
      process.stdout.write(msg.message.content);
      break;
      
    case 'response_end':
      console.log('\n--- 完成 ---');
      break;
      
    case 'tool_call':
      console.log('调用工具:', msg.message.tool_name);
      break;
  }
};

function sendMessage(content) {
  ws.send(JSON.stringify({
    id: crypto.randomUUID(),
    session_id: sessionId,
    message: {
      type: 'user_message',
      content: content
    },
    timestamp: Date.now()
  }));
}
```

## 平台适配器

### 已实现

- **WebSocket Spoke**：通用 WebSocket 客户端（Web、桌面应用等）
- **HTTP Spoke**：Webhook 回调（WhatsApp、Lark 等）
- **TUI Spoke**：终端界面

### 添加新适配器

实现 `SpokeAdapter` trait：

```rust
#[async_trait]
pub trait SpokeAdapter: Send + Sync {
    fn spoke_type(&self) -> SpokeType;
    async fn start(&self, message_tx: mpsc::UnboundedSender<(ClientInfo, GatewayMessage)>) -> Result<(), String>;
    async fn send(&self, client_id: &str, message: GatewayMessage) -> Result<(), String>;
    async fn stop(&self);
}
```

## 会话管理

- 会话以用户 ID 为维度，跨平台共享
- 默认会话超时：1 小时
- 支持多客户端同时连接同一会话
- 自动清理过期会话

## 配置

在 `config/bee.toml` 中：

```toml
[app]
max_context_turns = 20

[gateway]
bind_addr = "127.0.0.1:9000"
max_connections = 1000
session_timeout = 3600
```

或使用环境变量：

```bash
export GATEWAY_BIND=0.0.0.0:9000
```
