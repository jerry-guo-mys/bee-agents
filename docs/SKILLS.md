# 如何扩展 Bee 的 Skills（技能）

Bee 的技能系统支持**按需动态加载**，助手可以根据用户查询自动选择相关技能来增强能力。

---

## 一、技能目录结构

每个技能是一个独立的目录，包含：

```
config/skills/
├── viral/                  # 技能 ID（目录名）
│   ├── skill.toml         # 技能元数据（必需）
│   ├── capability.md      # 能力描述（必需，用于 LLM 选择）
│   ├── template.md        # 模板（可选）
│   └── script.py          # 脚本（可选）
├── writing/
│   ├── skill.toml
│   ├── capability.md
│   └── template.md
└── search/
    ├── skill.toml
    ├── capability.md
    └── search.py          # 可执行脚本
```

---

## 二、配置文件格式

### skill.toml（必需）

```toml
[skill]
id = "viral"
name = "爆款文章"
description = "高传播选题、标题、钩子与结构，适配公众号/小红书/抖音"
tags = ["写作", "营销", "自媒体"]

# 可选：关联脚本
script = "script.py"
script_type = "python"  # python / shell
```

字段说明：
- **id**: 唯一标识符（通常与目录名一致）
- **name**: 展示名称
- **description**: 简短描述，用于快速理解技能用途
- **tags**: 标签列表，用于分类和快速筛选
- **script**: 可执行脚本文件名（相对于技能目录）
- **script_type**: 脚本类型（python / shell）

### capability.md（必需）

能力描述文件，内容会被注入到 system prompt 中。建议结构：

```markdown
# 技能名称

## 核心能力
- 能力 1
- 能力 2

## 使用场景
- 场景 1
- 场景 2

## 输出格式
描述该技能产出的内容格式
```

### template.md（可选）

模板文件，提供常用格式和结构参考。当技能被选中时，模板内容也会注入到 prompt 中。

### script.py / script.sh（可选）

可执行脚本，用于扩展 Agent 能力。脚本通过工具插件机制被调用。

---

## 三、工作流程

### 1. 启动时加载

系统启动时自动扫描 `config/skills/` 目录，解析所有 `skill.toml` 并缓存描述信息：

```
[INFO] Loaded 3 skills
```

### 2. 按需选择

用户发送消息时，系统根据消息内容从缓存的技能描述中选择最相关的技能（默认最多 3 个）：

```
[INFO] Selected skills for query: ["viral", "writing"]
```

选择算法：
- 如果技能数量 ≤ 3，全部使用
- 否则，使用 LLM 根据用户查询和技能描述进行智能选择

### 3. 增强 Prompt

将选中技能的能力描述（capability.md）和模板（template.md）注入到 system prompt 中：

```
## 可用技能

### 爆款文章
（capability.md 内容）

#### 模板
（template.md 内容）
```

---

## 四、添加新技能

1. 在 `config/skills/` 下创建新目录（目录名即技能 ID）
2. 创建 `skill.toml` 填写元数据
3. 创建 `capability.md` 描述能力
4. 可选：创建 `template.md` 和脚本文件
5. 重启服务，技能自动加载

示例：创建一个翻译技能

```bash
mkdir -p config/skills/translate
```

`config/skills/translate/skill.toml`:
```toml
[skill]
id = "translate"
name = "翻译助手"
description = "多语言翻译，支持中英日韩等语言互译"
tags = ["翻译", "语言"]
```

`config/skills/translate/capability.md`:
```markdown
# 翻译技能

## 核心能力
- 多语言互译（中、英、日、韩等）
- 保持原文风格和语气
- 专业术语准确翻译

## 使用场景
- 文档翻译
- 实时对话翻译
- 技术文档本地化
```

---

## 五、API 使用

### 在代码中使用技能增强

```rust
use bee::agent::{process_message_with_skills, AgentComponents};

// 创建组件（自动加载技能）
let components = create_agent_components(&workspace, &system_prompt);

// 使用技能增强处理消息
let response = process_message_with_skills(
    &components,
    &mut context,
    "帮我写一篇爆款文章",
    event_tx,
    Some(&base_prompt),
    None,
    None,
).await?;
```

### 手动选择技能

```rust
use bee::skills::{SkillLoader, SkillSelector};

let loader = SkillLoader::from_default();
loader.load_all().await?;

let selector = SkillSelector::new(loader.cache(), llm);
let skills = selector.select("写一篇爆款文章").await;
let prompt = SkillSelector::build_skills_prompt(&skills);
```

### 按标签筛选

```rust
let skills = selector.filter_by_tags(&["写作", "营销"]).await;
```

### 按 ID 获取

```rust
let skills = selector.get_by_ids(&["viral", "writing"]).await;
```

---

## 六、现有技能

| ID | 名称 | 描述 |
|----|------|------|
| viral | 爆款文章 | 高传播选题、标题、钩子与结构 |
| writing | 英语写作 | 润色、扩写、纠错 |
| search | 智能搜索 | 调用搜索引擎获取实时信息 |

---

## 七、与旧版助手的关系

旧版通过 `config/assistants.toml` 定义的助手仍然有效，技能系统是对其的增强：

- **助手（Assistants）**：定义 AI 的角色和风格（system prompt）
- **技能（Skills）**：定义可选的能力模块，按需注入

一个助手可以使用多个技能，技能是跨助手共享的。
