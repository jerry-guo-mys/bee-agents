# Bee Web UI

在浏览器中与 Bee 对话，无需终端。

## 启动

```bash
# 需启用 web feature
cargo run --bin bee-web --features web

# 或先构建再运行
cargo build --bin bee-web --features web
./target/debug/bee-web
```

默认监听 **http://0.0.0.0:8080**，本机访问 http://127.0.0.1:8080 。

## 环境变量

与 TUI 一致，需至少配置一个 LLM：

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_API_KEY` | DeepSeek API Key（推荐） |
| `OPENAI_API_KEY` | OpenAI 兼容 API Key |
| （无） | 使用 Mock LLM（本地测试） |

## 功能

- **对话**：输入消息后点击「发送」或按 Enter，Bee 经 ReAct 循环后返回回复。
- **工具**：支持 cat、ls、shell、search、echo 等，与 TUI/WhatsApp 一致。
- **会话**：同一浏览器会话内保持上下文（短期 + 中期 + 长期记忆）。
- **健康检查**：GET `/api/health` 返回 `OK`。

## API

- **GET /**  
  返回聊天页面 HTML。

- **POST /api/chat**  
  请求体：`{ "message": "用户输入", "session_id": "可选" }`  
  响应：`{ "reply": "Bee 回复", "session_id": "会话 ID" }`  
  首次请求可不带 `session_id`，响应中会返回新会话 ID，后续请求带上以保持上下文。

- **GET /api/health**  
  返回 `OK`（纯文本）。

- **POST /api/config/reload**  
  重新加载配置并重建 Agent 组件（LLM/Planner 等），实现运行时多 LLM 后端切换；修改 `config/default.toml` 或环境变量后调用此接口即可生效，无需重启进程。

## 项目内文件

- **前端**：`static/index.html`（单页，内联 CSS/JS，编译时由 `include_str!` 打进二进制）。
- **后端**：`src/bin/web.rs`（Axum 路由、会话存储、调用 `bee::agent::process_message`）。

## 与 TUI 的区别

- 无流式输出：Web 端一次请求得到完整回复。
- 会话以 `session_id` 区分，存在服务端内存中，重启后清空。
- 端口固定为 8080（如需修改可改 `src/bin/web.rs` 中的 `addr`）。
