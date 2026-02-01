# Bee 记忆存储（Markdown 文件）

Bee 使用**简单透明的纯文本（Markdown）文件**存储短期与长期记忆，便于人工查看与版本管理。

## 目录结构

在 `workspace/memory/` 下：

```
memory/
├── logs/              # 短期记忆：按日期组织的对话日志
│   ├── 2025-02-01.md
│   ├── 2025-02-02.md
│   └── ...
└── long-term.md       # 长期记忆：持久知识、用户偏好等跨会话信息
```

## 短期记忆（日志）

- **路径**：`memory/logs/YYYY-MM-DD.md`
- **用途**：按日期记录当日对话，便于回溯与审计。
- **格式**：每次会话追加一段 Markdown，包含 Session ID、日期、User/Assistant 消息。

示例：

```markdown
## Session abc-123 (2025-02-01)

### User
今天天气怎么样？

### Assistant
我无法直接获取实时天气……

---

## Session def-456 (2025-02-01)
…
```

- **写入时机**：Web 端在保存会话到磁盘时（`save_session_to_disk`）同时追加到当日日志。

## 长期记忆（持久文件）

- **路径**：`memory/long-term.md`
- **用途**：存储跨会话的持久知识、用户偏好等，供 ReAct 检索后注入 Prompt。
- **格式**：按块追加，每块带时间戳标题。

示例：

```markdown
## 2025-02-01 12:00

用户偏好使用 Python，常用 fastapi。

## 2025-02-01 12:05

项目 Bee 的 workspace 位于 ~/projects/bee/workspace。
```

- **写入时机**：ReAct 循环在最终回复后调用 `push_to_long_term` 时。
- **检索**：当前实现为 **BM25 风格关键词检索**（按块切分、词重叠 + 文档长度归一化），启动时从文件加载到内存索引。

## 检索与扩展（向量 + BM25）

长期记忆的检索策略设计为可扩展：

1. **当前**：纯 BM25 风格关键词检索（`FileLongTerm`），无需外部服务。
2. **后续可扩展**：
   - **记忆碎片化与向量化**：对 `long-term.md` 的块（或更细的 chunk）做 Embedding，写入向量库（如 ChromaDB、LanceDB、Qdrant）。
   - **混合检索**：向量相似度 + BM25 关键词多路召回，再按 Recency / Relevance / Importance 加权排序。
   - **记忆注入**：检索结果作为「Relevant Past Knowledge」注入 system prompt，与现有 `long_term_section` 一致。

实现上可在 `memory/markdown_store.rs` 中增加「向量索引」层，或新增 `HybridLongTerm`，在不改变 `LongTermMemory` trait 的前提下接入向量库与 BM25。

## 配置与位置

- 记忆根目录由 **workspace** 推导：`workspace/memory/`。
- Web 端使用 `workspace` 为当前目录下的 `workspace`；TUI/WhatsApp 若未传 workspace，长期记忆仍为内存实现（`InMemoryLongTerm`），不写文件。

## 忽略与备份

- `workspace/` 已在 `.gitignore` 中，故 `workspace/memory/` 不会被提交。
- 如需备份，可直接复制 `memory/` 目录或对 `long-term.md` 与 `logs/*.md` 做版本管理。
