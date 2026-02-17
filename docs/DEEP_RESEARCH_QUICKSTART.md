# 深度研究功能快速开始 🚀

## 5 分钟上手指南

### 1. 启动 Bee

```bash
export DEEPSEEK_API_KEY=sk-xxx
cargo run
```

### 2. 使用深度研究

在对话中输入：

```
帮我研究一下 Rust 异步编程的最新发展，生成一份详细报告
```

Bee 会自动：
1. 使用 `deep_search` 分解问题并多轮搜索
2. 使用 `validate_source` 验证信息来源
3. 使用 `generate_report` 生成结构化报告
4. 可选：使用 `build_knowledge_graph` 构建知识图谱

### 3. 查看工具调用

在 Web UI 中可以看到完整的工具调用过程：

```
🔍 Deep Search: 分解为 4 个子问题
  - Rust async/await roadmap 2025 2026
  - Tokio new features latest version
  - Async traits stabilization status
  - Performance improvements benchmarks

📊 Search Round 1: 获取初始结果
✔️ Validate Source: wikipedia.org (trust: 0.9)
✔️ Validate Source: github.com (trust: 0.75)

📊 Search Round 2: 深入查询
  - Rust async performance comparison
  - Tokio vs async-std 2026

📄 Generate Report: 生成 Markdown 报告
  - Executive Summary
  - Key Findings
  - Analysis
  - Conclusions
  - References

🕸️ Build Knowledge Graph: 提取实体关系
  - Nodes: 12 entities
  - Edges: 8 relationships
```

---

## 高级用法

### 指定研究深度

```
研究量子计算对密码学的影响，进行 5 轮深度搜索
```

### 生成特定格式报告

```
将研究结果整理为 JSON 格式的报告
```

### 构建知识图谱

```
从这些研究结果中提取关键概念和它们的关系
```

---

## 配置文件

在 `config/default.toml` 中调整参数：

```toml
[tools.deep_research]
max_rounds = 5                    # 最大搜索轮数
trusted_domains = [
  "wikipedia.org", "arxiv.org", 
  "pubmed.gov", "scholar.google.com"
]
```

---

## API 调用示例

如果你使用 Web API：

```bash
# 深度搜索
curl -X POST http://localhost:8080/api/message \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "demo",
    "message": "研究 Rust 异步编程"
  }'

# 验证来源
curl -X POST http://localhost:8080/api/tool \
  -H "Content-Type: application/json" \
  -d '{
    "tool": "validate_source",
    "args": {"url": "https://arxiv.org/xxx"}
  }'

# 生成报告
curl -X POST http://localhost:8080/api/tool \
  -H "Content-Type: application/json" \
  -d '{
    "tool": "generate_report",
    "args": {
      "topic": "Rust Async",
      "findings": "...",
      "format": "markdown"
    }
  }'
```

---

## 性能基准

| 任务类型 | 预期时间 |
|---------|----------|
| 简单研究 (2 轮) | < 30 秒 |
| 标准研究 (3 轮) | < 60 秒 |
| 深度研究 (5 轮) | < 120 秒 |
| 报告生成 | < 30 秒 |
| 知识图谱构建 | < 20 秒 |

---

## 故障排除

### 问题：搜索结果为空
**解决**: 检查 `config/default.toml` 中的 `allowed_domains` 配置

### 问题：报告质量不佳
**解决**: 提供更详细的 findings，或增加研究轮数

### 问题：编译错误
**解决**: 确保 Rust 版本 >= 1.70
```bash
rustup update stable
cargo clean && cargo build
```

---

## 下一步

- 📖 阅读 [完整文档](docs/DEEP_RESEARCH.md)
- 🔧 尝试自定义可信域名列表
- 🎯 创建你的第一个深度研究报告
- 🤝 分享你的使用案例

---

*Happy Researching! 🐝📚*
