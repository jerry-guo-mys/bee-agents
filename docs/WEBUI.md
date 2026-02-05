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

默认监听 **http://0.0.0.0:8080**；端口可通过 `config/default.toml` 的 `[web].port` 或环境变量 `BEE_WEB_PORT` 修改。

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
- **会话**：同一浏览器会话内保持上下文（短期 + 中期 + 长期记忆）；会话按 `session_id` 持久化到 `workspace/sessions/*.json`，重启后可从磁盘恢复。
- **健康检查**：GET `/api/health` 返回 `OK`。
- **心跳**（可选）：若在 `config/default.toml` 中设置 `[heartbeat] enabled = true`，后台会按 `interval_secs` 定期执行自主「检查待办 / 反思」任务，结果写入 `workspace/memory/heartbeat_log.md` 并打日志。

## API

- **GET /**  
  返回聊天页面 HTML。

- **POST /api/chat**  
  请求体：`{ "message": "用户输入", "session_id": "可选" }`  
  响应：`{ "reply": "Bee 回复", "session_id": "会话 ID" }`  
  首次请求可不带 `session_id`，响应中会返回新会话 ID，后续请求带上以保持上下文。

- **POST /api/chat/stream**  
  流式聊天（**前端默认使用**）：请求体同 `/api/chat`，响应为 NDJSON 流（首行 `session_id`，后续为 `thinking` / `tool_call` / `message_chunk` / `message_done` 等），适合长回复与实时展示。

- **GET /api/health**  
  返回 `OK`（纯文本）。

- **POST /api/config/reload**  
  重新加载配置并重建 Agent 组件（LLM/Planner 等），实现运行时多 LLM 后端切换；修改 `config/default.toml` 或环境变量后调用此接口即可生效，无需重启进程。

- **POST /api/compact**  
  请求体：`{ "session_id": "..." }`。对指定会话执行上下文压缩（摘要写入长期记忆、当前消息替换为摘要），避免 token 溢出。

- **POST /api/memory/consolidate**  
  将近期短期日志归纳写入长期记忆（非 LLM 摘要）。

- **POST /api/memory/consolidate-llm**  
  查询参数：`?since_days=7`。对近期每日日志调用 LLM 做摘要后写入长期记忆。

## 项目内文件

- **前端**：`static/index.html`（单页，内联 CSS/JS，编译时由 `include_str!` 打进二进制）。
- **后端**：`src/bin/web.rs`（Axum 路由、会话存储、调用 `bee::agent::process_message`）。

## 与 TUI 的区别

- **流式**：前端默认使用 `/api/chat/stream`，长回复可边生成边展示；`/api/chat` 仍可用于一次取完整回复。
- 会话以 `session_id` 区分，存在服务端内存中，并会持久化到 `workspace/sessions/<session_id>.json`，重启后自动从磁盘加载已有会话。
- 端口由 `[web].port` 或 `BEE_WEB_PORT` 配置，默认 8080。
