# Bee 🐝

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.1.0-green.svg)](Cargo.toml)

> 高性能、安全且具备长期记忆的 Rust 个人智能体系统

Bee 是一个基于 ReAct 架构的智能体，支持多工具协作、分层记忆系统和多种交互界面（TUI/Web/WhatsApp）。

---

## ✨ 功能特性

- 🤖 **智能编排**: ReAct 循环 + Planner/Critic 双核心，自主规划与反思
- 🧠 **分层记忆**: 短期对话 + 中期工作区 + 长期持久化记忆
- 🛠️ **丰富工具**: 沙箱文件系统、Shell 白名单、Web 搜索、浏览器控制
- 💬 **多界面**: TUI 终端界面、Web 浏览器界面、WhatsApp 集成
- 🔒 **安全沙箱**: 受限文件系统访问、Shell 命令白名单机制
- 🔌 **多 LLM 支持**: DeepSeek/OpenAI 无缝切换，Mock 模式离线测试

---

## 🚀 快速开始

### 环境要求

- [Rust](https://rustup.rs/) 1.70+ (`rustup default stable`)
- DeepSeek 或 OpenAI API Key

### 安装运行

```bash
# 1. 克隆项目
git clone <repo-url>
cd bee

# 2. 设置 API Key（推荐 DeepSeek）
export DEEPSEEK_API_KEY=sk-xxx

# 3. 运行
cargo run
```

> 首次运行将自动创建 `workspace/` 目录和默认配置。

---

## 🖥️ 界面预览

### TUI 终端界面（默认）
```bash
cargo run              # 启动 TUI
cargo run --release    # 生产构建
```

**快捷键**:
| 快捷键 | 功能 |
|--------|------|
| `Enter` | 发送消息 |
| `Ctrl+C` | 取消当前生成 |
| `Ctrl+L` | 清空对话 |
| `Ctrl+Q` | 退出 |

### Web 界面
```bash
cargo run --bin bee-web --features web
```
访问 http://127.0.0.1:8080

### WhatsApp 集成
```bash
cargo run --bin bee-whatsapp --features whatsapp
```
> 需要公网 Webhook 回调域名（本地可用 ngrok）

---

## ⚙️ 配置

### 环境变量

| 变量 | 说明 | 优先级 |
|------|------|--------|
| `DEEPSEEK_API_KEY` | DeepSeek API Key | ⭐ 推荐 |
| `DEEPSEEK_MODEL` | `deepseek-chat`（默认）/ `deepseek-reasoner` | - |
| `OPENAI_API_KEY` | OpenAI API Key（gpt-4o-mini） | 备选 |
| （无需配置） | Mock LLM 离线模式 | 测试 |

**示例** - 使用 DeepSeek 思考模式：
```bash
export DEEPSEEK_API_KEY=sk-xxx
export DEEPSEEK_MODEL=deepseek-reasoner
cargo run
```

### 配置文件

- `config/default.toml` - 主配置（LLM 供应商、工具白名单等）
- `config/prompts/system.txt` - 系统 Prompt
- `workspace/` - 沙箱工作目录

### 多 LLM 切换

编辑 `config/default.toml`:
```toml
[llm]
provider = "deepseek"  # 或 "openai"
```

---

## 🏗️ 架构

```
┌─────────────────────────────────────────┐
│           交互层 (Interface)             │
│   ┌─────────┬──────────┬───────────┐   │
│   │ TUI     │ Web UI   │ WhatsApp  │   │
│   │ Ratatui │ Axum     │ Webhook   │   │
│   └────┬────┴────┬─────┴─────┬─────┘   │
└────────┼─────────┼───────────┼─────────┘
         │         │           │
         └─────────┴─────┬─────┘
                         ▼
┌─────────────────────────────────────────┐
│         核心编排 (Orchestrator)          │
│  Session Supervisor + Recovery Engine   │
└─────────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         ▼               ▼               ▼
┌─────────────┐   ┌──────────────┐   ┌──────────┐
│ 认知层       │   │    工具层    │   │  记忆层   │
│ Planner     │   │ 沙箱文件系统 │   │ 短期记忆  │
│ Critic      │   │ Shell 白名单 │   │ 中期记忆  │
│ ReAct Loop  │   │ Web 搜索     │   │ 长期记忆  │
└─────────────┘   └──────────────┘   └──────────┘
```

---

## 📁 项目结构

```
bee/
├── src/
│   ├── core/       # 编排、状态、恢复引擎
│   ├── llm/        # LLM 客户端 (OpenAI, DeepSeek, Mock)
│   ├── memory/     # 短期/中期/长期记忆系统
│   ├── react/      # Planner, Critic, ReAct 循环
│   ├── tools/      # 工具箱 (cat, ls, shell, search, echo, browser)
│   └── ui/         # TUI 界面 (Ratatui)
├── static/         # Web UI 前端 (index.html)
├── config/         # 配置与 Prompt 模板
└── workspace/      # 沙箱工作目录
```

---

## 📚 文档

### 项目文档
- [📖 使用文档](docs/使用文档.md) - 详细使用指南
- [🌐 Web UI 文档](docs/WEBUI.md) - Web 界面配置
- [💬 WhatsApp 文档](docs/WHATSAPP.md) - WhatsApp 集成指南
- [📑 文档导航](docs/README.md) - 完整文档索引

### 🤖 AI 行为改进系统
- [🎯 AI改进指南](AI_IMPROVEMENT_GUIDE.md) - **统一入口，从这里开始**
- [⚡ 快速参考](ai-quick-reference.md) - 日常交互速查卡
- [✅ 自检清单](ai-self-check-workflow.md) - 可执行检查清单
- [📋 改进方案](ai-improvement-plan.md) - 6大领域完整设计
- [📊 追踪记录](ai-improvement-tracking.md) - 效果验证表
- [🔧 监控指南](MONITORING_GUIDE.md) - 数据记录和报告
- [🚀 部署指南](DEPLOYMENT_GUIDE.md) - 生产级监控部署

---

## 🛠️ 开发

```bash
# 开发模式运行
cargo run

# 生产构建
cargo build --release

# 运行测试
cargo test

# 代码检查
cargo clippy
cargo fmt
```

### 功能开关

```bash
# Web 界面
cargo run --bin bee-web --features web

# WhatsApp 集成
cargo run --bin bee-whatsapp --features whatsapp

# 浏览器控制（需安装 Chrome/Chromium）
cargo run --features browser
```

---

## 🔒 安全特性

- **沙箱文件系统**: 只能访问 `workspace/` 目录
- **Shell 白名单**: 仅允许配置的命令（默认: ls, grep, cat, cargo 等）
- **域名白名单**: Web 搜索限制在允许域名内
- **API Key 隔离**: 环境变量管理，不写入配置文件

---

## 🤝 贡献

欢迎 Issue 和 PR！

1. Fork 项目
2. 创建分支 (`git checkout -b feature/amazing`)
3. 提交更改 (`git commit -m 'Add feature'`)
4. 推送分支 (`git push origin feature/amazing`)
5. 创建 Pull Request

---

## 📄 许可证

[MIT](LICENSE) © Bee Team

---

<div align="center">
  <sub>Built with 🦀 Rust</sub>
</div>
