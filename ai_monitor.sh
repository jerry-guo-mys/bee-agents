#!/bin/bash
# AI行为改进监控系统 - 快捷命令
# 使用方法: source ai_monitor.sh 或 . ./ai_monitor.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
alias aimon="python3 $SCRIPT_DIR/ai_monitor.py"
alias aireport="python3 $SCRIPT_DIR/ai_report.py"

# 快速记录错误
aimon-log() {
    python3 "$SCRIPT_DIR/ai_monitor.py" log
}

# 查看今日报告
aimon-today() {
    python3 "$SCRIPT_DIR/ai_monitor.py" daily
}

# 查看本周报告
aimon-week() {
    python3 "$SCRIPT_DIR/ai_monitor.py" weekly
}

# 查看KPI
aimon-kpi() {
    python3 "$SCRIPT_DIR/ai_monitor.py" kpi
}

# 生成可视化报告
aimon-visual() {
    python3 "$SCRIPT_DIR/ai_report.py"
}

# 快速记录今日数据
aimon-quick() {
    if [ $# -ne 5 ]; then
        echo "用法: aimon-quick [意图误解] [工具误用] [路径错误] [输出不当] [总交互]"
        return 1
    fi
    python3 "$SCRIPT_DIR/ai_monitor.py" quick "$1" "$2" "$3" "$4" "$5"
}

# 每日自检提示
aimon-daily-check() {
    echo "═══════════════════════════════════════════════════════"
    echo "          AI行为改进 - 每日自检清单"
    echo "═══════════════════════════════════════════════════════"
    echo ""
    echo "📊 数据记录"
    echo "  使用: aimon-quick [意图误解] [工具误用] [路径错误] [输出不当] [总交互]"
    echo "  示例: aimon-quick 0 1 0 0 10"
    echo ""
    echo "📝 详细记录"
    echo "  使用: aimon-log  (交互式记录具体错误)"
    echo ""
    echo "📈 查看报告"
    echo "  今日报告: aimon-today"
    echo "  本周报告: aimon-week"
    echo "  KPI汇总: aimon-kpi"
    echo "  可视化: aimon-visual"
    echo ""
    echo "═══════════════════════════════════════════════════════"
}

echo "AI行为改进监控系统快捷命令已加载"
echo "可用命令:"
echo "  aimon        - 查看完整命令列表"
echo "  aimon-log    - 交互式记录错误"
echo "  aimon-today  - 今日报告"
echo "  aimon-week   - 本周报告"
echo "  aimon-kpi    - KPI汇总"
echo "  aimon-visual - 生成可视化报告"
echo "  aimon-quick  - 快速记录数据"
echo "  aimon-daily-check - 显示每日自检提示"
