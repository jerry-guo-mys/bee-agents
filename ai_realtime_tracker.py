#!/usr/bin/env python3
"""
AI行为改进 - 实时监控系统
轻量级实时追踪，每次交互自动记录
"""

import json
import time
import asyncio
from datetime import datetime
from pathlib import Path
from typing import Optional, Dict, Any
from dataclasses import dataclass, asdict
from contextlib import contextmanager

DATA_DIR = Path(__file__).parent / "monitoring_data"
REALTIME_LOG = DATA_DIR / "realtime_sessions.json"


@dataclass
class InteractionSession:
    """单次交互会话记录"""
    session_id: str
    timestamp: str
    date: str
    user_input: str
    intent_understood: str
    tools_used: list
    files_accessed: list
    response_time_ms: int
    success: bool
    error_type: Optional[str] = None
    error_details: Optional[str] = None
    user_feedback: Optional[str] = None
    
    def to_dict(self) -> Dict:
        return asdict(self)


class RealtimeTracker:
    """实时追踪器 - 每次交互自动记录"""
    
    def __init__(self):
        self.current_session: Optional[InteractionSession] = None
        self.session_start_time: Optional[float] = None
        self.sessions: list = []
        self._load_sessions()
    
    def _load_sessions(self):
        """加载历史会话"""
        if REALTIME_LOG.exists():
            with open(REALTIME_LOG, 'r', encoding='utf-8') as f:
                self.sessions = json.load(f)
    
    def _save_sessions(self):
        """保存会话"""
        with open(REALTIME_LOG, 'w', encoding='utf-8') as f:
            json.dump(self.sessions, f, indent=2, ensure_ascii=False)
    
    @contextmanager
    def track_interaction(self, user_input: str):
        """上下文管理器 - 自动追踪交互"""
        self.start_session(user_input)
        try:
            yield self
            self.end_session(success=True)
        except Exception as e:
            self.end_session(
                success=False,
                error_type="执行错误",
                error_details=str(e)
            )
            raise
    
    def start_session(self, user_input: str):
        """开始追踪会话"""
        self.session_start_time = time.time()
        self.current_session = InteractionSession(
            session_id=f"{datetime.now().strftime('%Y%m%d_%H%M%S')}_{id(self)}",
            timestamp=datetime.now().isoformat(),
            date=datetime.now().strftime("%Y-%m-%d"),
            user_input=user_input[:200],  # 限制长度
            intent_understood="",
            tools_used=[],
            files_accessed=[],
            response_time_ms=0,
            success=False
        )
    
    def log_intent(self, understood: str):
        """记录理解到的意图"""
        if self.current_session:
            self.current_session.intent_understood = understood
    
    def log_tool_use(self, tool_name: str, params: Dict = None):
        """记录工具使用"""
        if self.current_session:
            self.current_session.tools_used.append({
                "tool": tool_name,
                "params": params,
                "timestamp": datetime.now().isoformat()
            })
    
    def log_file_access(self, file_path: str, operation: str):
        """记录文件访问"""
        if self.current_session:
            self.current_session.files_accessed.append({
                "path": file_path,
                "operation": operation,
                "timestamp": datetime.now().isoformat()
            })
    
    def end_session(self, success: bool = True, 
                   error_type: str = None, 
                   error_details: str = None,
                   user_feedback: str = None):
        """结束追踪会话"""
        if not self.current_session or not self.session_start_time:
            return
        
        # 计算响应时间
        duration = time.time() - self.session_start_time
        self.current_session.response_time_ms = int(duration * 1000)
        self.current_session.success = success
        self.current_session.error_type = error_type
        self.current_session.error_details = error_details
        self.current_session.user_feedback = user_feedback
        
        # 保存会话
        self.sessions.append(self.current_session.to_dict())
        self._save_sessions()
        
        # 打印实时反馈
        self._print_session_summary()
        
        # 重置状态
        self.current_session = None
        self.session_start_time = None
    
    def _print_session_summary(self):
        """打印会话摘要"""
        if not self.current_session:
            return
        
        s = self.current_session
        status = "✅ 成功" if s.success else "❌ 失败"
        
        print(f"\n{'='*60}")
        print(f"📊 实时追踪 - 会话完成")
        print(f"{'='*60}")
        print(f"状态: {status}")
        print(f"响应时间: {s.response_time_ms}ms")
        print(f"使用工具: {len(s.tools_used)}个")
        if s.error_type:
            print(f"错误类型: {s.error_type}")
        print(f"{'='*60}\n")
    
    def get_session_stats(self, date: str = None) -> Dict:
        """获取会话统计"""
        if date is None:
            date = datetime.now().strftime("%Y-%m-%d")
        
        day_sessions = [s for s in self.sessions if s['date'] == date]
        
        if not day_sessions:
            return {
                "total": 0,
                "success": 0,
                "failed": 0,
                "avg_response_time": 0,
                "error_types": {}
            }
        
        error_types = {}
        for s in day_sessions:
            if s.get('error_type'):
                error_types[s['error_type']] = error_types.get(s['error_type'], 0) + 1
        
        return {
            "total": len(day_sessions),
            "success": sum(1 for s in day_sessions if s['success']),
            "failed": sum(1 for s in day_sessions if not s['success']),
            "avg_response_time": sum(s['response_time_ms'] for s in day_sessions) / len(day_sessions),
            "error_types": error_types
        }
    
    def get_realtime_dashboard(self) -> str:
        """生成实时仪表板文本"""
        today = datetime.now().strftime("%Y-%m-%d")
        stats = self.get_session_stats(today)
        
        recent_sessions = [s for s in self.sessions if s['date'] == today][-5:]
        
        dashboard = f"""
╔══════════════════════════════════════════════════════════════╗
║              🤖 AI行为改进 - 实时仪表板 ({today})              ║
╚══════════════════════════════════════════════════════════════╝

📈 今日概览
─────────────────────────────────────────────────────────────
总交互:     {stats['total']}
成功:       {stats['success']} ✅
失败:       {stats['failed']} {'✅' if stats['failed'] == 0 else '❌'}
成功率:     {(stats['success']/stats['total']*100):.1f}% {'✓' if stats['success']/stats['total'] >= 0.95 else ''}
平均响应:   {stats['avg_response_time']:.0f}ms

❌ 错误分布
─────────────────────────────────────────────────────────────
"""
        if stats['error_types']:
            for error_type, count in sorted(stats['error_types'].items(), key=lambda x: x[1], reverse=True):
                bar = "█" * count
                dashboard += f"{error_type:12s}: {count:2d} {bar}\n"
        else:
            dashboard += "暂无错误 ✓\n"
        
        dashboard += """
🕐 最近会话
─────────────────────────────────────────────────────────────
"""
        for s in reversed(recent_sessions):
            status = "✓" if s['success'] else "✗"
            time_str = s['timestamp'][11:19]
            tools = ", ".join([t['tool'] for t in s['tools_used'][:2]])
            if len(s['tools_used']) > 2:
                tools += f" +{len(s['tools_used'])-2}"
            dashboard += f"[{time_str}] {status} {s['response_time_ms']:4d}ms | {tools or '无工具'}\n"
        
        dashboard += "\n" + "="*62 + "\n"
        
        return dashboard


# 全局追踪器实例
tracker = RealtimeTracker()


def track(func):
    """装饰器 - 自动追踪函数执行"""
    def wrapper(*args, **kwargs):
        user_input = str(args[0]) if args else ""
        with tracker.track_interaction(user_input) as t:
            result = func(*args, **kwargs)
            return result
    return wrapper


# 演示
if __name__ == "__main__":
    print("实时追踪器演示")
    print("="*60)
    
    # 模拟一个交互
    with tracker.track_interaction("帮我计算2+2") as t:
        # 模拟处理过程
        t.log_intent("数学计算请求")
        time.sleep(0.1)
        t.log_tool_use("bash", {"command": "echo '2+2' | bc"})
        time.sleep(0.05)
        t.log_file_access("/dev/null", "read")
        time.sleep(0.05)
    
    # 显示仪表板
    print(tracker.get_realtime_dashboard())
