#!/usr/bin/env python3
"""
AI行为改进监控系统
用于追踪错误、监控KPI、生成报告
"""

import json
import os
import sys
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional, Any
from dataclasses import dataclass, asdict
from collections import defaultdict
import statistics

# 配置
DATA_DIR = Path(__file__).parent / "monitoring_data"
LOG_FILE = DATA_DIR / "error_logs.json"
KPI_FILE = DATA_DIR / "kpi_data.json"
DAILY_LOG_FILE = DATA_DIR / "daily_logs.json"

# 确保数据目录存在
DATA_DIR.mkdir(exist_ok=True)


@dataclass
class ErrorLog:
    """错误日志条目"""
    timestamp: str
    date: str
    error_type: str  # 意图误解/工具误用/路径错误/输出不当/其他
    severity: str  # 低/中/高/严重
    scenario: str
    operation: str
    error_result: str
    user_feedback: str
    root_cause: str
    correction: str
    prevention: str
    
    def to_dict(self) -> Dict:
        return asdict(self)
    
    @classmethod
    def from_dict(cls, data: Dict) -> 'ErrorLog':
        return cls(**data)


@dataclass
class DailyMetrics:
    """每日指标"""
    date: str
    total_interactions: int = 0
    intent_misunderstandings: int = 0
    tool_misuses: int = 0
    path_errors: int = 0
    output_issues: int = 0
    user_corrections: int = 0
    avg_response_time: float = 0.0
    tasks_completed_first_try: int = 0
    tasks_total: int = 0
    notes: str = ""
    
    def to_dict(self) -> Dict:
        return asdict(self)
    
    @classmethod
    def from_dict(cls, data: Dict) -> 'DailyMetrics':
        return cls(**data)
    
    @property
    def completion_rate(self) -> float:
        if self.tasks_total == 0:
            return 0.0
        return (self.tasks_completed_first_try / self.tasks_total) * 100
    
    @property
    def error_rate(self) -> float:
        if self.total_interactions == 0:
            return 0.0
        total_errors = (self.intent_misunderstandings + self.tool_misuses + 
                       self.path_errors + self.output_issues)
        return (total_errors / self.total_interactions) * 100


class AIBehaviorMonitor:
    """AI行为监控器"""
    
    def __init__(self):
        self.error_logs: List[ErrorLog] = []
        self.daily_metrics: Dict[str, DailyMetrics] = {}
        self._load_data()
    
    def _load_data(self):
        """加载历史数据"""
        if LOG_FILE.exists():
            with open(LOG_FILE, 'r', encoding='utf-8') as f:
                data = json.load(f)
                self.error_logs = [ErrorLog.from_dict(e) for e in data]
        
        if DAILY_LOG_FILE.exists():
            with open(DAILY_LOG_FILE, 'r', encoding='utf-8') as f:
                data = json.load(f)
                self.daily_metrics = {
                    k: DailyMetrics.from_dict(v) for k, v in data.items()
                }
    
    def _save_data(self):
        """保存数据"""
        with open(LOG_FILE, 'w', encoding='utf-8') as f:
            json.dump([e.to_dict() for e in self.error_logs], f, indent=2, ensure_ascii=False)
        
        with open(DAILY_LOG_FILE, 'w', encoding='utf-8') as f:
            json.dump({k: v.to_dict() for k, v in self.daily_metrics.items()}, 
                     f, indent=2, ensure_ascii=False)
    
    def log_error(self, error_type: str, scenario: str, operation: str, 
                  error_result: str, user_feedback: str = "",
                  severity: str = "中", root_cause: str = "",
                  correction: str = "", prevention: str = "") -> None:
        """记录错误"""
        now = datetime.now()
        error = ErrorLog(
            timestamp=now.isoformat(),
            date=now.strftime("%Y-%m-%d"),
            error_type=error_type,
            severity=severity,
            scenario=scenario,
            operation=operation,
            error_result=error_result,
            user_feedback=user_feedback,
            root_cause=root_cause,
            correction=correction,
            prevention=prevention
        )
        self.error_logs.append(error)
        self._save_data()
        print(f"✓ 错误已记录: {error_type}")
    
    def get_or_create_daily_metrics(self, date: Optional[str] = None) -> DailyMetrics:
        """获取或创建每日指标"""
        if date is None:
            date = datetime.now().strftime("%Y-%m-%d")
        
        if date not in self.daily_metrics:
            self.daily_metrics[date] = DailyMetrics(date=date)
        
        return self.daily_metrics[date]
    
    def record_interaction(self, success: bool = True, response_time: float = 0,
                          error_type: Optional[str] = None, date: Optional[str] = None):
        """记录一次交互"""
        if date is None:
            date = datetime.now().strftime("%Y-%m-%d")
        
        metrics = self.get_or_create_daily_metrics(date)
        metrics.total_interactions += 1
        metrics.tasks_total += 1
        
        if success:
            metrics.tasks_completed_first_try += 1
        
        if error_type:
            if error_type == "意图误解":
                metrics.intent_misunderstandings += 1
            elif error_type == "工具误用":
                metrics.tool_misuses += 1
            elif error_type == "路径错误":
                metrics.path_errors += 1
            elif error_type == "输出不当":
                metrics.output_issues += 1
        
        if response_time > 0:
            # 计算新的平均响应时间
            old_total = metrics.avg_response_time * (metrics.total_interactions - 1)
            metrics.avg_response_time = (old_total + response_time) / metrics.total_interactions
        
        self._save_data()
    
    def generate_daily_report(self, date: Optional[str] = None) -> str:
        """生成每日报告"""
        if date is None:
            date = datetime.now().strftime("%Y-%m-%d")
        
        metrics = self.get_or_create_daily_metrics(date)
        day_errors = [e for e in self.error_logs if e.date == date]
        
        report = f"""
╔══════════════════════════════════════════════════════════════╗
║              AI行为改进 - 每日报告 ({date})              ║
╚══════════════════════════════════════════════════════════════╝

📊 核心指标
─────────────────────────────────────────────────────────────
总交互次数:     {metrics.total_interactions}
任务完成率:     {metrics.completion_rate:.1f}%
错误率:         {metrics.error_rate:.1f}%
平均响应时间:   {metrics.avg_response_time:.2f}秒

❌ 错误统计
─────────────────────────────────────────────────────────────
意图误解:       {metrics.intent_misunderstandings} 次
工具误用:       {metrics.tool_misuses} 次
路径错误:       {metrics.path_errors} 次
输出不当:       {metrics.output_issues} 次
用户纠正:       {metrics.user_corrections} 次

📋 详细错误记录
─────────────────────────────────────────────────────────────
"""
        
        if day_errors:
            for i, error in enumerate(day_errors, 1):
                report += f"""
错误 #{i}
  类型: {error.error_type} | 严重度: {error.severity}
  场景: {error.scenario[:60]}...
  操作: {error.operation[:60]}...
  根因: {error.root_cause[:60]}...
  预防措施: {error.prevention[:60]}...
"""
        else:
            report += "今日无错误记录 ✓\n"
        
        report += """
💡 今日总结
─────────────────────────────────────────────────────────────
"""
        if metrics.notes:
            report += metrics.notes + "\n"
        else:
            report += "（无备注）\n"
        
        report += "\n" + "="*62 + "\n"
        
        return report
    
    def generate_weekly_report(self, end_date: Optional[str] = None) -> str:
        """生成周报告"""
        if end_date is None:
            end_date = datetime.now().strftime("%Y-%m-%d")
        
        end = datetime.strptime(end_date, "%Y-%m-%d")
        start = end - timedelta(days=6)
        
        # 收集本周数据
        week_metrics = []
        week_errors = []
        
        for i in range(7):
            date = (start + timedelta(days=i)).strftime("%Y-%m-%d")
            if date in self.daily_metrics:
                week_metrics.append(self.daily_metrics[date])
            week_errors.extend([e for e in self.error_logs if e.date == date])
        
        # 计算汇总数据
        total_interactions = sum(m.total_interactions for m in week_metrics)
        total_errors = len(week_errors)
        avg_completion_rate = statistics.mean([m.completion_rate for m in week_metrics]) if week_metrics else 0
        avg_response_time = statistics.mean([m.avg_response_time for m in week_metrics if m.avg_response_time > 0]) if week_metrics else 0
        
        # 错误类型分布
        error_types = defaultdict(int)
        for e in week_errors:
            error_types[e.error_type] += 1
        
        report = f"""
╔══════════════════════════════════════════════════════════════╗
║            AI行为改进 - 周报告 ({start.strftime('%m-%d')} ~ {end.strftime('%m-%d')})           ║
╚══════════════════════════════════════════════════════════════╝

📈 周度概览
─────────────────────────────────────────────────────────────
统计周期:       {start.strftime('%Y-%m-%d')} 至 {end.strftime('%Y-%m-%d')}
总交互次数:     {total_interactions}
总错误数:       {total_errors}
平均完成率:     {avg_completion_rate:.1f}%
平均响应时间:   {avg_response_time:.2f}秒

📊 错误类型分布
─────────────────────────────────────────────────────────────
"""
        
        for error_type, count in sorted(error_types.items(), key=lambda x: x[1], reverse=True):
            percentage = (count / total_errors * 100) if total_errors > 0 else 0
            bar = "█" * int(percentage / 5)
            report += f"{error_type:12s}: {count:3d} 次 ({percentage:5.1f}%) {bar}\n"
        
        report += """
📅 每日趋势
─────────────────────────────────────────────────────────────
日期          交互数    错误数    完成率    响应时间
─────────────────────────────────────────────────────────────
"""
        
        for i in range(7):
            date = (start + timedelta(days=i)).strftime("%Y-%m-%d")
            if date in self.daily_metrics:
                m = self.daily_metrics[date]
                day_error_count = len([e for e in self.error_logs if e.date == date])
                report += f"{date}  {m.total_interactions:6d}    {day_error_count:6d}    {m.completion_rate:5.1f}%    {m.avg_response_time:6.2f}s\n"
            else:
                report += f"{date}       -         -         -         -\n"
        
        report += """
🔍 改进建议
─────────────────────────────────────────────────────────────
"""
        
        # 根据数据生成建议
        if total_errors > 0:
            top_error = max(error_types.items(), key=lambda x: x[1])
            report += f"1. 重点关注: {top_error[0]} 是本周主要问题，建议回顾相关改进措施\n"
        
        if avg_completion_rate < 90:
            report += "2. 任务完成率偏低，建议加强执行前的确认步骤\n"
        
        if avg_response_time > 30:
            report += "3. 响应时间较长，建议优化工具选择和执行效率\n"
        
        if total_errors == 0 and avg_completion_rate >= 95:
            report += "本周表现优秀！保持当前水平 ✓\n"
        
        report += "\n" + "="*62 + "\n"
        
        return report
    
    def get_kpi_summary(self) -> Dict[str, Any]:
        """获取KPI汇总"""
        if not self.daily_metrics:
            return {}
        
        dates = sorted(self.daily_metrics.keys())
        recent_7_days = dates[-7:] if len(dates) >= 7 else dates
        recent_30_days = dates[-30:] if len(dates) >= 30 else dates
        
        def calc_metrics(dates_list):
            interactions = sum(self.daily_metrics[d].total_interactions for d in dates_list)
            errors = sum(
                self.daily_metrics[d].intent_misunderstandings +
                self.daily_metrics[d].tool_misuses +
                self.daily_metrics[d].path_errors +
                self.daily_metrics[d].output_issues
                for d in dates_list
            )
            completion_rates = [self.daily_metrics[d].completion_rate for d in dates_list]
            
            return {
                "total_interactions": interactions,
                "total_errors": errors,
                "error_rate": (errors / interactions * 100) if interactions > 0 else 0,
                "avg_completion_rate": statistics.mean(completion_rates) if completion_rates else 0,
                "target_met": {
                    "error_rate": (errors / interactions * 100) < 5 if interactions > 0 else True,
                    "completion_rate": statistics.mean(completion_rates) >= 95 if completion_rates else False
                }
            }
        
        return {
            "last_7_days": calc_metrics(recent_7_days),
            "last_30_days": calc_metrics(recent_30_days),
            "all_time": calc_metrics(dates)
        }
    
    def interactive_log_error(self):
        """交互式记录错误"""
        print("\n" + "="*60)
        print("错误日志记录")
        print("="*60)
        
        error_types = ["意图误解", "工具误用", "路径错误", "输出不当", "其他"]
        severities = ["低", "中", "高", "严重"]
        
        print("\n错误类型:")
        for i, t in enumerate(error_types, 1):
            print(f"  {i}. {t}")
        type_idx = int(input("选择 (1-5): ")) - 1
        error_type = error_types[type_idx]
        
        print("\n严重度:")
        for i, s in enumerate(severities, 1):
            print(f"  {i}. {s}")
        sev_idx = int(input("选择 (1-4): ")) - 1
        severity = severities[sev_idx]
        
        scenario = input("\n场景描述 (用户请求): ")
        operation = input("我的操作: ")
        error_result = input("错误结果: ")
        user_feedback = input("用户反馈 (可选): ") or ""
        root_cause = input("根因分析: ")
        correction = input("修正方案: ")
        prevention = input("预防措施: ")
        
        self.log_error(
            error_type=error_type,
            severity=severity,
            scenario=scenario,
            operation=operation,
            error_result=error_result,
            user_feedback=user_feedback,
            root_cause=root_cause,
            correction=correction,
            prevention=prevention
        )
        
        print("\n✓ 错误记录完成！")


def main():
    """主函数"""
    monitor = AIBehaviorMonitor()
    
    if len(sys.argv) < 2:
        print("""
AI行为改进监控系统

用法: python ai_monitor.py [命令] [参数]

命令:
  log          交互式记录错误
  daily [日期] 生成每日报告 (默认今天)
  weekly [日期] 生成周报告 (默认本周)
  kpi          显示KPI汇总
  summary      显示简要统计
  quick        快速记录今日错误数

示例:
  python ai_monitor.py log
  python ai_monitor.py daily 2026-02-17
  python ai_monitor.py weekly
  python ai_monitor.py kpi
  python ai_monitor.py quick 2 1 0 0 5
        """)
        return
    
    command = sys.argv[1]
    
    if command == "log":
        monitor.interactive_log_error()
    
    elif command == "daily":
        date = sys.argv[2] if len(sys.argv) > 2 else None
        print(monitor.generate_daily_report(date))
    
    elif command == "weekly":
        end_date = sys.argv[2] if len(sys.argv) > 2 else None
        print(monitor.generate_weekly_report(end_date))
    
    elif command == "kpi":
        kpi = monitor.get_kpi_summary()
        print("\n" + "="*60)
        print("KPI 汇总")
        print("="*60)
        for period, data in kpi.items():
            print(f"\n【{period}】")
            if data:
                print(f"  总交互: {data['total_interactions']}")
                print(f"  总错误: {data['total_errors']}")
                print(f"  错误率: {data['error_rate']:.2f}% (目标: <5%)")
                print(f"  完成率: {data['avg_completion_rate']:.2f}% (目标: >95%)")
                print(f"  目标达成: {'✓' if all(data['target_met'].values()) else '✗'}")
    
    elif command == "summary":
        total_errors = len(monitor.error_logs)
        total_days = len(monitor.daily_metrics)
        total_interactions = sum(m.total_interactions for m in monitor.daily_metrics.values())
        
        print(f"\n📊 监控数据汇总")
        print("="*60)
        print(f"监控天数: {total_days}")
        print(f"总交互数: {total_interactions}")
        print(f"总错误数: {total_errors}")
        print(f"错误率: {(total_errors/total_interactions*100):.2f}%" if total_interactions > 0 else "无数据")
    
    elif command == "quick":
        # 快速记录: quick [意图误解数] [工具误用数] [路径错误数] [输出不当数] [总交互数]
        if len(sys.argv) >= 6:
            date = datetime.now().strftime("%Y-%m-%d")
            metrics = monitor.get_or_create_daily_metrics(date)
            metrics.intent_misunderstandings = int(sys.argv[2])
            metrics.tool_misuses = int(sys.argv[3])
            metrics.path_errors = int(sys.argv[4])
            metrics.output_issues = int(sys.argv[5])
            metrics.total_interactions = int(sys.argv[6]) if len(sys.argv) > 6 else 0
            monitor._save_data()
            print(f"✓ 已记录 {date} 的数据")
        else:
            print("用法: python ai_monitor.py quick [意图误解] [工具误用] [路径错误] [输出不当] [总交互]")
    
    else:
        print(f"未知命令: {command}")
        print("使用 'python ai_monitor.py' 查看帮助")


if __name__ == "__main__":
    main()
