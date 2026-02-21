# 技能系统（Skills）

技能是一组能力描述、模板和脚本的集合。助手可以按需从技能库中选择相关技能，动态增强其能力。

## 目录结构

```
config/skills/
├── viral/
│   ├── skill.toml      # 技能元数据
│   ├── capability.md   # 能力描述（用于 LLM 选择）
│   ├── template.md     # 模板（可选）
│   └── script.py       # 脚本（可选）
├── writing/
│   ├── skill.toml
│   ├── capability.md
│   └── template.md
└── ...
```

## 配置格式

### skill.toml

```toml
[skill]
id = "viral"
name = "爆款文章"
description = "高传播选题、标题、钩子与结构，适配公众号/小红书/抖音"
tags = ["写作", "营销", "自媒体"]

# 可选：关联的脚本
script = "script.py"
script_type = "python"
```

### capability.md

能力描述，用于 LLM 在选择技能时了解该技能的功能。建议包含：

- 核心能力列表
- 使用场景
- 输出格式

### template.md（可选）

模板文件，提供常用格式和结构，会被注入到 prompt 中。

### script.py / script.sh（可选）

可执行脚本，用于扩展 Agent 能力。

## 工作流程

1. **启动时加载**：系统扫描 `config/skills/` 下的所有子目录，解析 `skill.toml` 并缓存描述信息
2. **按需选择**：用户发送消息时，根据消息内容从缓存的描述中选择最相关的技能（默认最多 3 个）
3. **增强 Prompt**：将选中技能的能力描述和模板注入到 system prompt 中

## 添加新技能

1. 在 `config/skills/` 下创建新目录（目录名即为技能 ID）
2. 创建 `skill.toml` 填写元数据
3. 创建 `capability.md` 描述能力
4. 可选：创建 `template.md` 和脚本文件
5. 重启服务，技能自动加载

## 示例技能

- `viral/` - 爆款文章写作
- `writing/` - 英语写作
- `search/` - 智能搜索（带脚本）
