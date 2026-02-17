# Bee 项目文档导航 📚

> 快速找到你需要的文档

---

## 🚀 新手上路

| 文档 | 阅读时间 | 内容 |
|------|---------|------|
| [README.md](../README.md) | 5分钟 | 项目概览、快速开始 |
| [使用文档.md](使用文档.md) | 15分钟 | 详细使用指南、配置说明 |
| [ARCHITECTURE_ANALYSIS.md](ARCHITECTURE_ANALYSIS.md) | 20分钟 | 架构设计分析 |

---

## 🤖 AI 行为改进系统

### 核心文档

| 文档 | 用途 | 使用频率 |
|------|------|---------|
| [🎯 ai-improvement-plan.md](../ai-improvement-plan.md) | **完整改进方案** - 6大领域的系统性设计 | 首次详细阅读 |
| [⚡ ai-quick-reference.md](../ai-quick-reference.md) | **快速参考卡** - 日常交互查阅 | 每次响应前 |
| [✅ ai-self-check-workflow.md](../ai-self-check-workflow.md) | **自检工作流** - 可执行的检查清单 | 每次任务后 |
| [📊 ai-improvement-tracking.md](../ai-improvement-tracking.md) | **实施追踪** - 指标记录和效果验证 | 每日/每周 |

### 监控工具

| 文档 | 用途 |
|------|------|
| [MONITORING_GUIDE.md](../MONITORING_GUIDE.md) | 手动监控系统使用指南 |
| [DEPLOYMENT_GUIDE.md](../DEPLOYMENT_GUIDE.md) | 生产级实时监控系统部署 |

---

## 🏗️ 技术文档

### 架构与设计

| 文档 | 内容 |
|------|------|
| [Rust个人智能体系统(Bee)-架构设计白皮书.md](Rust个人智能体系统(Bee)-架构设计白皮书.md) | 系统架构设计白皮书（中文） |
| [EVOLUTION_DESIGN.md](EVOLUTION_DESIGN.md) | 系统演进设计 |
| [EVOLUTION.md](EVOLUTION.md) | 系统演进计划 |

### 功能模块

| 文档 | 内容 |
|------|------|
| [WEBUI.md](WEBUI.md) | Web 界面配置和使用 |
| [WHATSAPP.md](WHATSAPP.md) | WhatsApp 集成指南 |

### 开发参考

| 文档 | 内容 |
|------|------|
| [LEARNINGS.md](LEARNINGS.md) | 项目学习笔记 |
| [MEMORY.md](MEMORY.md) | 记忆系统设计 |

---

## 📋 快速参考索引

### 按场景查找

**🎯 我要改进AI行为**
→ [ai-quick-reference.md](../ai-quick-reference.md) → [ai-self-check-workflow.md](../ai-self-check-workflow.md)

**📊 我要追踪改进效果**
→ [MONITORING_GUIDE.md](../MONITORING_GUIDE.md) → [ai-improvement-tracking.md](../ai-improvement-tracking.md)

**🚀 我要部署监控系统**
→ [DEPLOYMENT_GUIDE.md](../DEPLOYMENT_GUIDE.md)

**🤔 我要了解项目架构**
→ [ARCHITECTURE_ANALYSIS.md](ARCHITECTURE_ANALYSIS.md) → [Rust个人智能体系统(Bee)-架构设计白皮书.md](Rust个人智能体系统(Bee)-架构设计白皮书.md)

**💻 我要使用某个界面**
→ [使用文档.md](使用文档.md) → [WEBUI.md](WEBUI.md) / [WHATSAPP.md](WHATSAPP.md)

---

## 🗂️ 文档地图

```
docs/
├── README.md (本文档) ← 你在这里
├── 使用文档.md
├── ARCHITECTURE_ANALYSIS.md
├── Rust个人智能体系统(Bee)-架构设计白皮书.md
├── EVOLUTION_DESIGN.md
├── EVOLUTION.md
├── WEBUI.md
├── WHATSAPP.md
├── LEARNINGS.md
└── MEMORY.md

根目录/
├── README.md
├── ai-improvement-plan.md        ← 完整改进方案
├── ai-quick-reference.md          ← 速查卡
├── ai-self-check-workflow.md      ← 自检清单
├── ai-improvement-tracking.md     ← 追踪表
├── MONITORING_GUIDE.md            ← 监控指南
└── DEPLOYMENT_GUIDE.md            ← 部署指南
```

---

## 📝 文档规范

### 文件名约定
- 中文文档：使用描述性中文名（如`使用文档.md`）
- AI改进文档：以`ai-`前缀（如`ai-improvement-plan.md`）
- 英文文档：使用大写下划线（如`ARCHITECTURE_ANALYSIS.md`）

### 文档格式
- 使用 Markdown 格式
- 包含清晰的标题层级（H1-H3）
- 使用表格展示对比信息
- 代码块标明语言类型

### 维护责任
- AI改进相关文档：由AI助手维护
- 项目架构文档：由开发者维护
- 使用指南：共同维护

---

## 🔍 搜索提示

在VS Code中搜索文档内容：
```bash
# 搜索包含特定内容的文档
grep -r "关键词" docs/

# 查找最近修改的文档
ls -lt docs/*.md | head -5
```

---

*最后更新：2026-02-17*  
*维护者：AI助手 + 开发团队*
