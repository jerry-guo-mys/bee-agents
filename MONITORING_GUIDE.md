# AI行为改进监控系统 - 使用指南

## 📦 系统组成

| 文件 | 功能 |
|------|------|
| `ai_monitor.py` | 核心监控和数据记录工具 |
| `ai_report.py` | 可视化报告生成器 |
| `ai_monitor.sh` | Bash快捷命令（可选） |
| `monitoring_data/` | 数据存储目录 |

## 🚀 快速开始

### 1. 基本命令

```bash
# 查看所有命令
python3 ai_monitor.py

# 交互式记录错误
python3 ai_monitor.py log

# 生成今日报告
python3 ai_monitor.py daily

# 生成本周报告
python3 ai_monitor.py weekly

# 查看KPI汇总
python3 ai_monitor.py kpi

# 快速记录数据
python3 ai_monitor.py quick 0 1 0 0 10
# 参数: [意图误解] [工具误用] [路径错误] [输出不当] [总交互]
```

### 2. 使用Bash快捷命令（推荐）

```bash
# 加载快捷命令
source ai_monitor.sh

# 然后可以使用
aimon-log        # 记录错误
aimon-today      # 今日报告
aimon-week       # 本周报告
aimon-kpi        # KPI汇总
aimon-visual     # 可视化报告
aimon-quick 0 1 0 0 10  # 快速记录
```

## 📊 监控指标

### 核心KPI

| 指标 | 目标 | 说明 |
|------|------|------|
| 错误率 | <5% | (总错误/总交互) × 100% |
| 任务完成率 | >95% | 首次成功完成的任务比例 |
| 平均响应时间 | <30s | 从请求到响应的时间 |

### 错误类型

- **意图误解**: 理解错用户意图
- **工具误用**: 选择了不合适的工具
- **路径错误**: 操作不存在的文件路径
- **输出不当**: 回答不够实用或完整

## 📝 记录流程

### 场景1: 每天结束时快速记录

```bash
# 回顾今天的情况，记录数据
aimon-quick 0 1 0 0 10
# 表示: 0个意图误解, 1个工具误用, 0个路径错误, 0个输出问题, 总共10次交互
```

### 场景2: 详细记录重要错误

```bash
aimon-log

# 系统会交互式询问:
# - 错误类型
# - 严重度
# - 场景描述
# - 你的操作
# - 错误结果
# - 用户反馈
# - 根因分析
# - 修正方案
# - 预防措施
```

### 场景3: 查看改进效果

```bash
# 查看今日情况
aimon-today

# 查看本周趋势
aimon-week

# 查看可视化图表
aimon-visual  # 会自动打开浏览器
```

## 📈 报告解读

### 每日报告包含

1. **核心指标**: 交互数、完成率、错误率、响应时间
2. **错误统计**: 各类型错误数量和用户纠正次数
3. **详细记录**: 当天所有错误的详细信息
4. **今日总结**: 备注和改进要点

### 周报告包含

1. **周度概览**: 汇总统计数据
2. **错误分布**: 可视化图表显示各类型错误占比
3. **每日趋势**: 7天数据对比
4. **改进建议**: 基于数据的自动建议

### 可视化报告

生成包含以下图表的HTML页面：
- 错误率趋势线（含目标线）
- 任务完成率趋势
- 每日交互量柱状图
- 错误类型分布饼图
- 近期错误记录列表

## 🎯 持续改进流程

### 每日
1. 工作结束后快速记录当天数据
2. 如有重要错误，详细记录
3. 查看今日报告，反思改进

### 每周
1. 周一生成上周报告
2. 分析错误类型分布
3. 识别需要重点改进的领域
4. 更新改进计划

### 每月
1. 汇总月度数据
2. 评估目标达成情况
3. 调整改进策略
4. 更新文档和流程

## 💡 最佳实践

### DO ✅
- 每天记录数据，保持连续性
- 重要错误详细记录，便于分析
- 定期查看报告，发现趋势
- 根据数据调整行为

### DON'T ❌
- 不记录就忘记分析
- 只记数量不分析原因
- 忽视用户反馈
- 数据造假

## 🔧 数据管理

### 数据存储
- 所有数据存储在 `monitoring_data/` 目录
- JSON格式，便于查看和处理
- 可以手动编辑（谨慎操作）

### 备份建议
```bash
# 定期备份数据
cp -r monitoring_data monitoring_data_backup_$(date +%Y%m%d)

# 或者使用git
# 将monitoring_data添加到.gitignore
```

### 数据导出
```python
# 可以编写脚本导出为Excel/CSV
import json
import pandas as pd

with open('monitoring_data/daily_logs.json') as f:
    data = json.load(f)

df = pd.DataFrame.from_dict(data, orient='index')
df.to_csv('daily_metrics.csv')
```

## 🐛 故障排除

### 问题1: 命令找不到
```bash
# 确保Python3已安装
python3 --version

# 使用完整路径
python3 /path/to/ai_monitor.py
```

### 问题2: 权限错误
```bash
# 添加执行权限
chmod +x ai_monitor.py ai_report.py
```

### 问题3: 数据文件损坏
```bash
# 备份后重建
mv monitoring_data monitoring_data_backup
mkdir monitoring_data
# 重新记录数据
```

## 📚 相关文档

- `ai-improvement-plan.md` - 完整改进方案
- `ai-self-check-workflow.md` - 自检工作流
- `ai-quick-reference.md` - 快速参考卡
- `ai-improvement-tracking.md` - 实施追踪表

## 🎓 示例工作流

```bash
# 1. 早上加载快捷命令
source ai_monitor.sh

# 2. 工作过程中如有错误，立即记录
aimon-log

# 3. 下班前快速记录今日数据
aimon-quick 0 0 1 0 15

# 4. 查看今日报告
aimon-today

# 5. 周五生成本周可视化报告
aimon-visual

# 6. 查看KPI趋势
aimon-kpi
```

---

**开始使用**: 运行 `python3 ai_monitor.py` 或 `source ai_monitor.sh`

**获取帮助**: 运行 `aimon-daily-check` 查看每日自检提示
