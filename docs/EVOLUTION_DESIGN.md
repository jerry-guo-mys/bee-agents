# Bee 自主迭代能力设计文档

## 1. 概述

自主迭代能力让 Bee 能够自我分析、改进和优化自身代码，实现真正的自我进化。

## 2. 核心概念

### 2.1 迭代循环

```
┌─────────────────────────────────────────────────────────────────┐
│                     自主迭代循环 (Self-Evolution Loop)            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │
│  │   自我分析    │ -> │  方案生成    │ -> │  代码修改    │       │
│  │  Self-Reflect│    │   Plan       │    │   Execute    │       │
│  └──────────────┘    └──────────────┘    └──────────────┘       │
│         ^                                        │               │
│         │          ┌──────────────┐             │               │
│         └────────- │  测试验证    │ <-------------┘               │
│                    │   Verify     │                               │
│                    └──────────────┘                               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 模块职责

| 模块 | 职责 | 对应文件 |
|------|------|----------|
| `SelfReflection` | 分析代码、识别问题、评估质量 | `src/evolution/reflection.rs` |
| `ImprovementPlanner` | 基于分析生成改进方案 | `src/evolution/planner.rs` |
| `CodeModifier` | 执行代码修改（调用工具） | `src/evolution/modifier.rs` |
| `TestValidator` | 验证修改、运行测试 | `src/evolution/validator.rs` |
| `EvolutionLoop` | 控制整个迭代流程 | `src/evolution/loop_.rs` |
| `EvolutionTools` | 代码修改工具集 | `src/tools/code_*.rs` |

## 3. 新工具集

### 3.1 代码分析工具
- `code_analyze`: 静态分析代码文件
- `code_lint`: 运行 cargo clippy 等检查
- `code_metrics`: 计算代码复杂度指标

### 3.2 代码修改工具
- `code_read`: 读取代码文件
- `code_edit`: 修改代码片段（类似 edit 工具）
- `code_write`: 写入新文件
- `code_replace`: 替换代码块
- `code_grep`: 搜索代码

### 3.3 测试验证工具
- `test_run`: 运行测试
- `test_check`: 检查编译
- `test_benchmark`: 性能测试

## 4. 迭代策略

### 4.1 触发条件
1. **定期触发**: 心跳机制（Heartbeat）
2. **问题触发**: 发现错误/失败时
3. **目标触发**: 用户设定改进目标

### 4.2 改进类型
1. **Bug修复**: 修复发现的错误
2. **性能优化**: 提升运行效率
3. **代码重构**: 改善代码结构
4. **功能增强**: 添加新功能
5. **文档完善**: 改进注释和文档

### 4.3 安全机制
1. **沙箱限制**: 只能在 workspace 内修改
2. **版本控制**: 自动 git commit 备份
3. **回滚机制**: 失败时自动回滚
4. **人工确认**: 关键修改需要确认

## 5. 记忆与经验积累

### 5.1 经验记录
- `evolution/lessons.md`: 迭代经验教训
- `evolution/success_patterns.md`: 成功改进模式
- `evolution/failure_log.md`: 失败记录及原因

### 5.2 改进追踪
- 记录每次迭代的改进点
- 追踪改进效果
- 积累可复用的改进模板

## 6. 集成点

### 6.1 与现有系统集成
1. **复用 ReAct 循环**: 使用现有 loop_.rs 框架
2. **复用工具系统**: 注册新工具到 ToolRegistry
3. **复用记忆系统**: 将经验写入长期记忆
4. **复用 Critic**: 评估改进方案质量

### 6.2 配置扩展
```toml
[evolution]
# 已存在
auto_lesson_on_hallucination = true
record_tool_success = false

# 新增
enabled = true                    # 启用自主迭代
max_iterations = 10               # 单次最大迭代次数
target_score_threshold = 0.8      # 目标质量分数
auto_commit = true               # 自动 git commit
require_approval = false         # 是否需要人工确认
focus_areas = ["performance", "readability"]  # 重点关注领域
```

## 7. 实现优先级

### Phase 1: 基础工具集
- [x] 代码读取工具 (code_read)
- [x] 代码搜索工具 (code_grep)
- [ ] 代码编辑工具 (code_edit)
- [ ] 代码写入工具 (code_write)

### Phase 2: 分析与验证
- [ ] 代码分析模块
- [ ] 测试验证模块
- [ ] 质量评估模块

### Phase 3: 迭代循环
- [ ] 改进方案生成
- [ ] 迭代循环控制器
- [ ] 经验记录系统

### Phase 4: 集成与优化
- [ ] 心跳触发机制
- [ ] 用户交互界面
- [ ] 安全与回滚
