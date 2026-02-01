# Bee - Rust 个人智能体系统

高性能、安全且具备长期记忆的个人智能体，基于白皮书架构实现。

## 快速开始

```bash
# 设置 DeepSeek API Key（推荐）
export DEEPSEEK_API_KEY=sk-xxx
cargo run
```

详细说明请参阅 **[使用文档](docs/使用文档.md)**。

## 架构

- **交互层**: Ratatui TUI
- **核心编排**: Orchestrator + Session Supervisor + Recovery Engine
- **认知层**: Planner + Critic + ReAct 循环
- **工具**: 沙箱文件系统 (cat, ls)、Shell 白名单、Search/Web、Echo
- **记忆**: 短期 (Conversation) + 中期 (Working) + 长期 (InMemoryLongTerm)

## 构建

```bash
# 确保 Rust 已安装 (rustup default stable)
cargo build --release
```

## 运行

```bash
# 从项目根目录运行
cargo run

# 或
./target/release/bee
```

### 环境变量

| 变量 | 说明 | 优先级 |
|------|------|--------|
| `DEEPSEEK_API_KEY` | DeepSeek API Key | 1 |
| `DEEPSEEK_MODEL` | 可选，`deepseek-chat`（默认）或 `deepseek-reasoner` | - |
| `OPENAI_API_KEY` | OpenAI API Key，使用 gpt-4o-mini | 2 |
| （无） | 使用 Mock LLM（无需网络，用于测试） | 3 |

使用 DeepSeek：

```bash
export DEEPSEEK_API_KEY=sk-xxx
# 可选：使用思考模式
export DEEPSEEK_MODEL=deepseek-reasoner
cargo run
```

### 快捷键

| 快捷键 | 功能 |
|--------|------|
| Enter | 发送消息 |
| Ctrl+C | 取消当前生成 |
| Ctrl+L | 清空对话 |
| Ctrl+Q | 退出 |

## 配置与多 LLM

- **配置**: `config/default.toml`（启动时加载，修改后需重启生效）
- **多 LLM**: 在 `config/default.toml` 中设置 `[llm] provider = "deepseek"` 或 `"openai"`，配合对应 API Key 即可切换后端
- **Shell 白名单**: `[tools.shell] allowed_commands` 控制允许的命令（如 ls, grep, cargo）
- **Search 域名白名单**: `[tools.search] allowed_domains` 控制可抓取的域名
- **长期记忆**: 默认启用 InMemoryLongTerm（关键词检索），重要回复会自动写入并在后续对话中检索
- `config/prompts/system.txt` - 系统 Prompt
- `workspace/` - 沙箱文件系统根目录

## Web UI

浏览器访问 Bee，无需终端：

```bash
cargo run --bin bee-web --features web
```

在浏览器打开 http://127.0.0.1:8080 即可对话，支持工具调用与多轮会话（按 session 保持上下文）。详见 [docs/WEBUI.md](docs/WEBUI.md)。

## WhatsApp 集成（可选）

WhatsApp 需要**公网 Webhook 回调域名**（本地可用 ngrok），无法提供时可跳过。

通过 WhatsApp 与 Bee 对话：

```bash
cargo run --bin bee-whatsapp --features whatsapp
```

需配置 `WHATSAPP_ACCESS_TOKEN`、`WHATSAPP_PHONE_NUMBER_ID` 等环境变量，详见 [docs/WHATSAPP.md](docs/WHATSAPP.md)。

## 项目结构

```
bee/
├── src/
│   ├── core/       # 编排、状态、恢复
│   ├── llm/        # LLM 客户端 (OpenAI, Mock)
│   ├── memory/     # 短期/中期记忆、持久化
│   ├── react/      # Planner, Critic, ReAct 循环
│   ├── tools/      # 工具箱 (cat, ls, shell, search, echo)
│   └── ui/         # TUI 界面
├── static/         # Web UI 前端 (index.html)
├── config/         # 配置与 Prompt
└── workspace/      # 沙箱工作目录
```
