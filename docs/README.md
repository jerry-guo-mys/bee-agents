# Bee 项目文档导航

> Rust 个人智能体系统 - 高性能、安全且具备长期记忆

---

## 系统架构

### Hub-and-Spoke（轮毂式）架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Hub（轮毂/中枢）                               │
│                          核心运行时 Runtime                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │ LLM 路由网关 │  │  记忆系统   │  │  意图识别   │  │  决策引擎   │    │
│  │ 模型选择    │  │ 短期对话日志 │  │ 快速规则    │  │ ReAct 循环  │    │
│  │ 负载均衡    │  │ 长期文件索引 │  │ LLM 分类    │  │ 规划/执行   │    │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
              ┌─────────────────────┴─────────────────────┐
              │                                           │
              ▼                                           ▼
┌─────────────────────────────┐         ┌─────────────────────────────────┐
│  通讯端点 Communication      │         │  能力端点 Capability             │
├─────────────────────────────┤         ├─────────────────────────────────┤
│ • Web (WebSocket)           │         │ • Skills 技能                   │
│ • TUI (终端)                │         │ • 本地工具 (cat/ls/shell)       │
│ • WhatsApp / Telegram       │         │ • API 插件 (search/browser)     │
│ • 飞书 Lark                 │         │ • 自动化脚本 (Python/Shell)     │
│ • HTTP API                  │         │                                 │
└─────────────────────────────┘         └─────────────────────────────────┘
```

### 模块结构

```
src/
├── main.rs          # TUI 入口
├── lib.rs           # 库导出
├── agent.rs         # 无头 Agent 运行时
├── config.rs        # 配置加载
│
├── gateway/         # Hub-and-Spoke 网关架构
│   ├── hub.rs       # 中枢：LLM 路由、会话管理
│   ├── intent.rs    # 意图识别
│   ├── runtime.rs   # Agent 核心运行时
│   ├── session.rs   # 跨平台会话管理
│   ├── spoke.rs     # 通讯/能力端点
│   └── message.rs   # 消息协议
│
├── core/            # 核心模块
│   ├── orchestrator.rs  # 编排器
│   ├── state.rs         # 状态管理
│   └── recovery.rs      # 错误恢复
│
├── llm/             # LLM 客户端
│   ├── client.rs    # OpenAI 兼容 API
│   └── deepseek.rs  # DeepSeek 特化
│
├── memory/          # 记忆系统
│   ├── short_term.rs    # 短期记忆
│   ├── long_term.rs     # 长期记忆
│   └── vector.rs        # 向量检索
│
├── react/           # ReAct 决策引擎
│   ├── planner.rs   # 规划器
│   ├── critic.rs    # 评审器
│   └── context.rs   # 上下文管理
│
├── skills/          # 技能系统
│   ├── loader.rs    # 技能加载
│   └── selector.rs  # 技能选择
│
├── tools/           # 工具箱
│   ├── cat.rs       # 文件读取
│   ├── shell.rs     # 命令执行
│   ├── search.rs    # 网页搜索
│   ├── browser.rs   # 浏览器控制 (语义快照)
│   └── code_*.rs    # 代码编辑工具
│
└── ui/              # TUI 界面
    └── app.rs       # Ratatui 应用
```

---

## 快速开始

### 运行方式

```bash
# 1. TUI 终端界面（默认）
cargo run

# 2. Web 界面
cargo run --bin bee-web --features web
# 访问 http://localhost:3000

# 3. Hub 网关服务（支持多平台接入）
cargo run --bin bee-gateway --features gateway
# WebSocket: ws://localhost:9000

# 4. WhatsApp 集成
cargo run --bin bee-whatsapp --features whatsapp

# 5. 飞书 Lark 集成
cargo run --bin bee-lark --features lark
```

### 配置文件

```toml
# config/bee.toml

[app]
max_context_turns = 20

[llm]
provider = "deepseek"
model = "deepseek-reasoner"
base_url = "https://api.deepseek.com"

[tools.shell]
allowed_commands = ["ls", "cat", "grep", "cargo", "git"]

[tools.search]
allowed_domains = ["github.com", "docs.rs", "stackoverflow.com"]

[memory]
vector_enabled = true
embedding_model = "text-embedding-3-small"
```

### 环境变量

```bash
export DEEPSEEK_API_KEY=sk-xxx
export OPENAI_API_KEY=sk-xxx
export GATEWAY_BIND=0.0.0.0:9000
```

---

## 核心功能

### 1. 技能系统 (Skills)

技能是可热加载的能力模块，包含能力描述、模板和脚本。

```
config/skills/
├── claude/           # Claude AI 最佳实践
│   ├── skill.toml
│   ├── capability.md
│   └── template.md
├── openclaw/         # 法律助手
│   ├── skill.toml
│   ├── capability.md
│   └── template.md
└── search/           # 智能搜索
    ├── skill.toml
    ├── capability.md
    └── search.py
```

### 2. 浏览器语义快照 (Semantic Snapshots)

通过无障碍树优化网页浏览，降低 Token 开销：

```json
{"action": "navigate", "url": "https://example.com"}
```

返回结构化语义：
```
[1] button: "Submit"
[2] textbox: "Search"
[3] link: "About Us"
```

精准交互：
```json
{"action": "click", "ref": 1}
{"action": "type", "ref": 2, "text": "hello"}
```

### 3. 意图识别

自动识别用户意图，路由到合适的能力：

| 意图 | 示例 | 推荐工具 |
|------|------|---------|
| `Search` | "搜索 Rust 异步" | search, deep_search |
| `Code` | "写一个排序函数" | code_write, code_edit |
| `Browse` | "打开 https://..." | browser |
| `Shell` | "运行 cargo test" | shell |
| `Memory` | "回忆之前说过的" | 长期记忆检索 |

### 4. 跨平台会话

同一用户在不同平台的对话共享上下文：

```
用户 A → Web      ─┐
用户 A → WhatsApp ─┼─→ 同一个 Session
用户 A → TUI      ─┘
```

---

## 文档索引

### 新手上路

| 文档 | 内容 |
|------|------|
| [使用文档.md](使用文档.md) | 详细使用指南 |
| [GATEWAY.md](GATEWAY.md) | Hub-and-Spoke 网关架构 |

### 架构设计

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE_ANALYSIS.md](ARCHITECTURE_ANALYSIS.md) | 架构设计分析 |
| [Rust个人智能体系统(Bee)-架构设计白皮书.md](Rust个人智能体系统(Bee)-架构设计白皮书.md) | 架构白皮书 |
| [EVOLUTION.md](EVOLUTION.md) | 系统演进计划 |
| [MEMORY.md](MEMORY.md) | 记忆系统设计 |

### 集成指南

| 文档 | 内容 |
|------|------|
| [WEBUI.md](WEBUI.md) | Web 界面 |
| [WHATSAPP.md](WHATSAPP.md) | WhatsApp 集成 |
| [GATEWAY.md](GATEWAY.md) | 网关架构与 WebSocket 协议 |

### AI 行为改进

| 文档 | 用途 |
|------|------|
| [ai-improvement-plan.md](../ai-improvement-plan.md) | 完整改进方案 |
| [ai-quick-reference.md](../ai-quick-reference.md) | 快速参考卡 |
| [ai-self-check-workflow.md](../ai-self-check-workflow.md) | 自检工作流 |

---

## 开发指南

### 构建命令

```bash
# 检查
cargo check
cargo clippy

# 测试
cargo test
cargo test test_name -- --nocapture

# 格式化
cargo fmt

# 发布构建
cargo build --release
```

### 添加新工具

1. 在 `src/tools/` 创建工具文件
2. 实现 `Tool` trait
3. 在 `ToolRegistry` 中注册

```rust
#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "Tool description" }
    async fn execute(&self, args: Value) -> Result<String, String> {
        // 实现
    }
}
```

### 添加新技能

在 `config/skills/{skill_id}/` 创建：

```
skill.toml      # 元数据
capability.md   # 能力描述（用于 LLM 选择）
template.md     # 模板（可选）
script.py       # 脚本（可选）
```

---

## 文档地图

```
项目根目录/
├── README.md                    # 项目入口
├── AGENTS.md                    # AI Agent 开发指南
├── ai-improvement-*.md          # AI 行为改进文档
│
├── docs/
│   ├── README.md (本文档)       # 文档导航
│   ├── GATEWAY.md               # 网关架构
│   ├── WEBUI.md                 # Web 界面
│   ├── WHATSAPP.md              # WhatsApp 集成
│   ├── MEMORY.md                # 记忆系统
│   ├── EVOLUTION.md             # 演进计划
│   └── ...
│
├── config/
│   ├── bee.toml                 # 主配置
│   ├── prompts/                 # 系统提示词
│   └── skills/                  # 技能定义
│
└── src/                         # 源代码
```

---

*最后更新：2026-02-21*  
*维护者：Bee 团队*
