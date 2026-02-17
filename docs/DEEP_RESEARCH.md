# 深度研究功能 📚

> 让 Bee 像专业研究员一样进行深度调查和多轮信息整合

---

## 🎯 功能概述

深度研究功能使 Bee 能够：

- **自主分解复杂问题** - 将大型研究问题拆解为多个可搜索的子问题
- **多轮迭代搜索** - 基于前一轮结果生成新的深入查询
- **信息源可信度评估** - 自动评估来源的可靠性
- **结构化报告生成** - 生成带引用和结论的完整研究报告
- **知识图谱构建** - 从研究结果中提取实体和关系

---

## 🛠️ 新增工具

### 1. `deep_search` - 深度搜索

**用途**: 对复杂主题进行多轮自主研究

**参数**:
```json
{
  "topic": "研究问题",
  "max_rounds": 3  // 可选，默认 3，最大 5
}
```

**示例**:
```json
{
  "topic": "量子计算对密码学的影响",
  "max_rounds": 4
}
```

**返回**:
```json
{
  "topic": "量子计算对密码学的影响",
  "summary": "200-300 字的综合摘要",
  "key_findings": ["发现 1", "发现 2", "发现 3"],
  "total_sources": 12,
  "follow_up_questions": ["后续问题 1", "后续问题 2"]
}
```

---

### 2. `validate_source` - 信息源验证

**用途**: 评估网页来源的可信度

**参数**:
```json
{
  "url": "https://example.com/article",
  "content": "可选的内容片段"
}
```

**返回**:
```json
{
  "url": "https://example.com/article",
  "trust_score": 0.85,
  "credibility": "high",
  "recommendation": "reliable source for research"
}
```

**可信度评级**:
- `high` (≥0.8): 学术期刊、政府网站、知名百科
- `medium` (0.6-0.8): 技术社区、知名媒体
- `low` (<0.6): 个人博客、未验证来源

---

### 3. `generate_report` - 报告生成

**用途**: 将研究结果整理为结构化报告

**参数**:
```json
{
  "topic": "研究主题",
  "findings": "研究数据和分析",
  "format": "markdown"  // 或 "json"
}
```

**Markdown 报告结构**:
```markdown
# [报告标题]

## Executive Summary
[简要概述]

## Key Findings
- 发现 1
- 发现 2

## Analysis
[详细分析]

## Conclusions
[主要结论]

## Recommendations
[可操作的建议]

## References
- 来源 1
- 来源 2
```

---

### 4. `build_knowledge_graph` - 知识图谱构建

**用途**: 从研究信息中提取实体和关系

**参数**:
```json
{
  "topic": "主题",
  "information": "要分析的文本"
}
```

**返回**:
```json
{
  "topic": "主题",
  "graph": {
    "nodes": [
      {"id": "entity1", "label": "实体标签", "type": "concept", "properties": {}}
    ],
    "edges": [
      {"source": "entity1", "target": "entity2", "relationship": "related_to"}
    ]
  },
  "visualization_hint": "Use a force-directed graph layout for visualization"
}
```

---

## 📖 使用示例

### 示例 1: 技术研究

**用户**: "帮我研究 Rust 异步编程的最新发展"

**Bee** (使用 deep_search):
```json
{
  "tool": "deep_search",
  "args": {
    "topic": "Rust async/await latest developments 2025 2026",
    "max_rounds": 4
  }
}
```

**研究过程**:
1. 分解为子问题:
   - Rust async roadmap 2025
   - Tokio new features
   - Async traits stabilization
   - Performance improvements

2. 多轮搜索整合结果

3. 生成综合报告

---

### 示例 2: 竞品分析

**用户**: "分析一下主要的 AI Agent 框架"

**Bee** 工作流:
```
1. deep_search → 获取市场信息
2. validate_source → 验证来源可信度
3. build_knowledge_graph → 提取框架特性和关系
4. generate_report → 生成竞品分析报告
```

---

### 示例 3: 学术论文调研

**用户**: "帮我收集关于 transformer 架构优化的论文"

**Bee**:
```json
{
  "tool": "deep_search",
  "args": {
    "topic": "transformer architecture optimization techniques 2025 2026",
    "max_rounds": 5
  }
}
```

**后续**:
```json
{
  "tool": "generate_report",
  "args": {
    "topic": "Transformer Optimization Survey",
    "findings": "[研究结果]",
    "format": "markdown"
  }
}
```

---

## ⚙️ 配置说明

在 `config/default.toml` 中添加:

```toml
[tools.deep_research]
max_rounds = 5                    # 最大搜索轮数
max_results_per_round = 3         # 每轮最大结果数
trusted_domains = [               # 可信域名列表
  "wikipedia.org", "arxiv.org", 
  "pubmed.gov", "scholar.google.com",
  "github.com", "stackoverflow.com"
]
```

---

## 🔧 最佳实践

### 1. 选择合适的 max_rounds
- 简单查询：2-3 轮
- 中等复杂度：3-4 轮
- 深度研究：5 轮（最大值）

### 2. 验证关键来源
对重要信息使用 `validate_source` 确保可靠性

### 3. 生成结构化报告
研究完成后使用 `generate_report` 整理发现

### 4. 构建知识图谱
对复杂主题使用 `build_knowledge_graph` 可视化关系

---

## 📊 性能指标

| 指标 | 目标值 |
|------|--------|
| 单轮搜索时间 | < 15 秒 |
| 完整研究 (3 轮) | < 60 秒 |
| 报告生成 | < 30 秒 |
| 来源验证 | < 2 秒 |

---

## 🐛 故障排除

### 问题：搜索结果太少
**解决**: 增加 `max_rounds` 或扩大查询范围

### 问题：报告质量不佳
**解决**: 提供更详细的 `findings` 输入

### 问题：来源验证分数低
**解决**: 交叉验证多个来源，优先选择可信域名

---

## 🚀 进阶用法

### 组合使用多个工具

```rust
// 1. 深度研究
let research = deep_search.execute(json!({
    "topic": "AI safety research",
    "max_rounds": 4
})).await?;

// 2. 验证关键来源
let validation = validator.execute(json!({
    "url": "https://arxiv.org/xxx"
})).await?;

// 3. 构建知识图谱
let graph = kg_builder.execute(json!({
    "topic": "AI Safety",
    "information": research
})).await?;

// 4. 生成最终报告
let report = generator.execute(json!({
    "topic": "AI Safety Survey",
    "findings": format!("{research}\n{graph}"),
    "format": "markdown"
})).await?;
```

---

## 📝 更新日志

### v0.1.0 (2026-02-17)
- ✅ 实现 `deep_search` 工具
- ✅ 实现 `validate_source` 工具
- ✅ 实现 `generate_report` 工具
- ✅ 实现 `build_knowledge_graph` 工具
- ✅ 添加配置文件支持
- ✅ 集成到 Agent 工具链

---

## 🤝 贡献

欢迎提交功能建议和 Bug 报告！

**待实现功能**:
- [ ] 支持 PDF 论文解析
- [ ] 多语言研究支持
- [ ] 引用格式导出（BibTeX）
- [ ] 实时研究进度可视化

---

*最后更新：2026-02-17*
