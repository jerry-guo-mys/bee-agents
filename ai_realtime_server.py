#!/usr/bin/env python3
"""
AI行为改进 - 生产级实时监控系统
WebSocket服务 + 实时数据流 + 报警机制
"""

import asyncio
import json
import sqlite3
import time
import uuid
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional, Set
from dataclasses import dataclass, asdict
from contextlib import asynccontextmanager
import threading

# WebSocket支持
try:
    import websockets
    from websockets.server import WebSocketServerProtocol
    WEBSOCKET_AVAILABLE = True
except ImportError:
    WEBSOCKET_AVAILABLE = False
    print("警告: websockets库未安装，运行: pip install websockets")

# 配置
DATA_DIR = Path(__file__).parent / "monitoring_data"
DB_FILE = DATA_DIR / "realtime_monitoring.db"
ALERT_CONFIG_FILE = DATA_DIR / "alert_config.json"

# 确保目录存在
DATA_DIR.mkdir(exist_ok=True)


@dataclass
class RealtimeMetrics:
    """实时指标数据包"""
    timestamp: str
    session_id: str
    user_input: str
    intent: str
    tools_used: List[Dict]
    response_time_ms: int
    success: bool
    error_type: Optional[str]
    error_severity: Optional[str]
    
    def to_json(self) -> str:
        return json.dumps(asdict(self), ensure_ascii=False)


class DatabaseManager:
    """SQLite数据库管理器 - 高效存储和查询"""
    
    def __init__(self, db_path: Path):
        self.db_path = db_path
        self._init_db()
    
    def _init_db(self):
        """初始化数据库表"""
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            
            # 会话表
            cursor.execute('''
                CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT UNIQUE,
                    timestamp TEXT,
                    date TEXT,
                    user_input TEXT,
                    intent TEXT,
                    response_time_ms INTEGER,
                    success BOOLEAN,
                    error_type TEXT,
                    error_severity TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            ''')
            
            # 工具使用表
            cursor.execute('''
                CREATE TABLE IF NOT EXISTS tool_usage (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT,
                    tool_name TEXT,
                    params TEXT,
                    timestamp TEXT,
                    FOREIGN KEY (session_id) REFERENCES sessions(session_id)
                )
            ''')
            
            # 实时统计表（用于快速查询）
            cursor.execute('''
                CREATE TABLE IF NOT EXISTS realtime_stats (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    date TEXT UNIQUE,
                    total_sessions INTEGER DEFAULT 0,
                    success_count INTEGER DEFAULT 0,
                    error_count INTEGER DEFAULT 0,
                    avg_response_time REAL DEFAULT 0,
                    intent_errors INTEGER DEFAULT 0,
                    tool_errors INTEGER DEFAULT 0,
                    path_errors INTEGER DEFAULT 0,
                    output_errors INTEGER DEFAULT 0,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            ''')
            
            # 报警记录表
            cursor.execute('''
                CREATE TABLE IF NOT EXISTS alerts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    alert_type TEXT,
                    severity TEXT,
                    message TEXT,
                    metric_value REAL,
                    threshold REAL,
                    acknowledged BOOLEAN DEFAULT FALSE,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            ''')
            
            # 创建索引优化查询
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_sessions_date ON sessions(date)')
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_sessions_success ON sessions(success)')
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_sessions_error ON sessions(error_type)')
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_tool_usage_session ON tool_usage(session_id)')
            
            conn.commit()
    
    def insert_session(self, metrics: RealtimeMetrics):
        """插入会话数据"""
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            
            # 插入会话
            cursor.execute('''
                INSERT INTO sessions 
                (session_id, timestamp, date, user_input, intent, response_time_ms, 
                 success, error_type, error_severity)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ''', (
                metrics.session_id,
                metrics.timestamp,
                datetime.now().strftime('%Y-%m-%d'),
                metrics.user_input,
                metrics.intent,
                metrics.response_time_ms,
                metrics.success,
                metrics.error_type,
                metrics.error_severity
            ))
            
            # 插入工具使用记录
            for tool in metrics.tools_used:
                cursor.execute('''
                    INSERT INTO tool_usage (session_id, tool_name, params, timestamp)
                    VALUES (?, ?, ?, ?)
                ''', (
                    metrics.session_id,
                    tool.get('tool', ''),
                    json.dumps(tool.get('params', {})),
                    tool.get('timestamp', '')
                ))
            
            conn.commit()
            
            # 更新实时统计
            self._update_realtime_stats(conn)
    
    def _update_realtime_stats(self, conn: sqlite3.Connection):
        """更新实时统计数据"""
        cursor = conn.cursor()
        today = datetime.now().strftime('%Y-%m-%d')
        
        # 计算今日统计
        cursor.execute('''
            SELECT 
                COUNT(*) as total,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as success_count,
                SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as error_count,
                AVG(response_time_ms) as avg_response,
                SUM(CASE WHEN error_type = '意图误解' THEN 1 ELSE 0 END) as intent_errors,
                SUM(CASE WHEN error_type = '工具误用' THEN 1 ELSE 0 END) as tool_errors,
                SUM(CASE WHEN error_type = '路径错误' THEN 1 ELSE 0 END) as path_errors,
                SUM(CASE WHEN error_type = '输出不当' THEN 1 ELSE 0 END) as output_errors
            FROM sessions
            WHERE date = ?
        ''', (today,))
        
        row = cursor.fetchone()
        
        # 插入或更新统计
        cursor.execute('''
            INSERT INTO realtime_stats 
            (date, total_sessions, success_count, error_count, avg_response_time,
             intent_errors, tool_errors, path_errors, output_errors)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(date) DO UPDATE SET
                total_sessions = excluded.total_sessions,
                success_count = excluded.success_count,
                error_count = excluded.error_count,
                avg_response_time = excluded.avg_response_time,
                intent_errors = excluded.intent_errors,
                tool_errors = excluded.tool_errors,
                path_errors = excluded.path_errors,
                output_errors = excluded.output_errors,
                updated_at = CURRENT_TIMESTAMP
        ''', (today, row[0], row[1], row[2], row[3], row[4], row[5], row[6], row[7]))
        
        conn.commit()
    
    def get_realtime_stats(self, date: str = None) -> Dict:
        """获取实时统计"""
        if date is None:
            date = datetime.now().strftime('%Y-%m-%d')
        
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute('''
                SELECT * FROM realtime_stats WHERE date = ?
            ''', (date,))
            
            row = cursor.fetchone()
            if row:
                return {
                    'date': row[1],
                    'total_sessions': row[2],
                    'success_count': row[3],
                    'error_count': row[4],
                    'avg_response_time': round(row[5], 2) if row[5] else 0,
                    'intent_errors': row[6],
                    'tool_errors': row[7],
                    'path_errors': row[8],
                    'output_errors': row[9],
                    'success_rate': round(row[3] / row[2] * 100, 2) if row[2] > 0 else 0,
                    'error_rate': round(row[4] / row[2] * 100, 2) if row[2] > 0 else 0
                }
            return {}
    
    def get_recent_sessions(self, limit: int = 10) -> List[Dict]:
        """获取最近会话"""
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute('''
                SELECT * FROM sessions 
                ORDER BY timestamp DESC 
                LIMIT ?
            ''', (limit,))
            
            columns = [description[0] for description in cursor.description]
            return [dict(zip(columns, row)) for row in cursor.fetchall()]
    
    def get_error_distribution(self, days: int = 7) -> Dict[str, int]:
        """获取错误分布"""
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            start_date = (datetime.now() - timedelta(days=days)).strftime('%Y-%m-%d')
            
            cursor.execute('''
                SELECT error_type, COUNT(*) as count
                FROM sessions
                WHERE date >= ? AND error_type IS NOT NULL
                GROUP BY error_type
            ''', (start_date,))
            
            return {row[0]: row[1] for row in cursor.fetchall()}


class AlertManager:
    """报警管理器 - 自动检测异常并发送通知"""
    
    def __init__(self, db_manager: DatabaseManager):
        self.db = db_manager
        self.config = self._load_config()
        self.alert_history: List[Dict] = []
    
    def _load_config(self) -> Dict:
        """加载报警配置"""
        if ALERT_CONFIG_FILE.exists():
            with open(ALERT_CONFIG_FILE, 'r', encoding='utf-8') as f:
                return json.load(f)
        
        # 默认配置
        default_config = {
            'error_rate_threshold': 5.0,  # 错误率超过5%报警
            'response_time_threshold': 30000,  # 响应时间超过30秒报警
            'consecutive_errors_threshold': 3,  # 连续3次错误报警
            'alert_cooldown_minutes': 15,  # 报警冷却时间
            'enabled': True,
            'webhook_url': None,  # 可配置Webhook
            'email_notifications': False
        }
        
        with open(ALERT_CONFIG_FILE, 'w', encoding='utf-8') as f:
            json.dump(default_config, f, indent=2)
        
        return default_config
    
    def check_alerts(self, metrics: RealtimeMetrics) -> Optional[Dict]:
        """检查是否需要报警"""
        if not self.config.get('enabled', True):
            return None
        
        alerts = []
        
        # 检查错误率
        stats = self.db.get_realtime_stats()
        if stats and stats.get('error_rate', 0) > self.config['error_rate_threshold']:
            alert = {
                'type': 'error_rate_high',
                'severity': 'warning',
                'message': f"错误率过高: {stats['error_rate']:.1f}% (阈值: {self.config['error_rate_threshold']}%)",
                'metric_value': stats['error_rate'],
                'threshold': self.config['error_rate_threshold'],
                'timestamp': datetime.now().isoformat()
            }
            if self._should_send_alert(alert):
                alerts.append(alert)
        
        # 检查响应时间
        if metrics.response_time_ms > self.config['response_time_threshold']:
            alert = {
                'type': 'response_time_high',
                'severity': 'warning',
                'message': f"响应时间过长: {metrics.response_time_ms}ms (阈值: {self.config['response_time_threshold']}ms)",
                'metric_value': metrics.response_time_ms,
                'threshold': self.config['response_time_threshold'],
                'timestamp': datetime.now().isoformat()
            }
            if self._should_send_alert(alert):
                alerts.append(alert)
        
        # 检查严重错误
        if metrics.error_severity in ['高', '严重']:
            alert = {
                'type': 'critical_error',
                'severity': 'critical',
                'message': f"发生严重错误: {metrics.error_type} - {metrics.user_input[:50]}...",
                'metric_value': 1,
                'threshold': 0,
                'timestamp': datetime.now().isoformat()
            }
            if self._should_send_alert(alert):
                alerts.append(alert)
        
        # 保存报警记录
        for alert in alerts:
            self._save_alert(alert)
        
        return alerts[0] if alerts else None
    
    def _should_send_alert(self, alert: Dict) -> bool:
        """检查是否应该发送报警（避免重复）"""
        cooldown = timedelta(minutes=self.config['alert_cooldown_minutes'])
        
        for history_alert in self.alert_history:
            if (history_alert['type'] == alert['type'] and
                datetime.now() - datetime.fromisoformat(history_alert['timestamp']) < cooldown):
                return False
        
        return True
    
    def _save_alert(self, alert: Dict):
        """保存报警到数据库"""
        with sqlite3.connect(self.db.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute('''
                INSERT INTO alerts (alert_type, severity, message, metric_value, threshold)
                VALUES (?, ?, ?, ?, ?)
            ''', (alert['type'], alert['severity'], alert['message'], 
                  alert['metric_value'], alert['threshold']))
            conn.commit()
        
        self.alert_history.append(alert)


class RealtimeMonitorServer:
    """实时监控系统 - WebSocket服务"""
    
    def __init__(self, host: str = "localhost", port: int = 8765):
        self.host = host
        self.port = port
        self.db = DatabaseManager(DB_FILE)
        self.alert_manager = AlertManager(self.db)
        self.connected_clients: Set[WebSocketServerProtocol] = set()
        self.running = False
    
    async def handle_client(self, websocket: WebSocketServerProtocol, path: str):
        """处理客户端连接"""
        self.connected_clients.add(websocket)
        print(f"✓ 客户端连接: {websocket.remote_address}")
        
        try:
            # 发送初始数据
            await self.send_initial_data(websocket)
            
            # 保持连接并处理消息
            async for message in websocket:
                await self.process_message(websocket, message)
        
        except websockets.exceptions.ConnectionClosed:
            print(f"✗ 客户端断开: {websocket.remote_address}")
        finally:
            self.connected_clients.discard(websocket)
    
    async def process_message(self, websocket: WebSocketServerProtocol, message: str):
        """处理客户端消息"""
        try:
            data = json.loads(message)
            msg_type = data.get('type')
            
            if msg_type == 'metrics':
                # 接收指标数据
                metrics = RealtimeMetrics(**data['data'])
                await self.handle_metrics(metrics)
            
            elif msg_type == 'request_stats':
                # 请求统计数据
                stats = self.db.get_realtime_stats()
                await websocket.send(json.dumps({
                    'type': 'stats_update',
                    'data': stats
                }))
            
            elif msg_type == 'request_history':
                # 请求历史记录
                sessions = self.db.get_recent_sessions(data.get('limit', 10))
                await websocket.send(json.dumps({
                    'type': 'history_update',
                    'data': sessions
                }))
        
        except Exception as e:
            print(f"处理消息错误: {e}")
            await websocket.send(json.dumps({
                'type': 'error',
                'message': str(e)
            }))
    
    async def handle_metrics(self, metrics: RealtimeMetrics):
        """处理指标数据"""
        # 保存到数据库
        self.db.insert_session(metrics)
        
        # 检查报警
        alert = self.alert_manager.check_alerts(metrics)
        
        # 广播给所有客户端
        message = {
            'type': 'metrics_update',
            'data': asdict(metrics)
        }
        
        if alert:
            message['alert'] = alert
        
        await self.broadcast(message)
    
    async def send_initial_data(self, websocket: WebSocketServerProtocol):
        """发送初始数据给新客户端"""
        stats = self.db.get_realtime_stats()
        sessions = self.db.get_recent_sessions(10)
        error_dist = self.db.get_error_distribution(7)
        
        await websocket.send(json.dumps({
            'type': 'initial_data',
            'data': {
                'stats': stats,
                'recent_sessions': sessions,
                'error_distribution': error_dist
            }
        }))
    
    async def broadcast(self, message: Dict):
        """广播消息给所有客户端"""
        if not self.connected_clients:
            return
        
        message_str = json.dumps(message, ensure_ascii=False)
        
        # 发送给所有连接的客户端
        disconnected = set()
        for client in self.connected_clients:
            try:
                await client.send(message_str)
            except websockets.exceptions.ConnectionClosed:
                disconnected.add(client)
        
        # 清理断开的客户端
        self.connected_clients -= disconnected
    
    async def start(self):
        """启动服务器"""
        if not WEBSOCKET_AVAILABLE:
            print("错误: 请先安装websockets库")
            print("运行: pip install websockets")
            return
        
        self.running = True
        print(f"🚀 实时监控系统启动")
        print(f"   WebSocket地址: ws://{self.host}:{self.port}")
        print(f"   数据库: {DB_FILE}")
        print(f"   按Ctrl+C停止")
        
        async with websockets.serve(self.handle_client, self.host, self.port):
            await asyncio.Future()  # 永久运行
    
    def run(self):
        """运行服务器（同步接口）"""
        try:
            asyncio.run(self.start())
        except KeyboardInterrupt:
            print("\n✓ 服务器已停止")


# 便捷函数
def send_metrics(metrics_data: Dict, websocket_url: str = "ws://localhost:8765"):
    """发送指标到监控服务器"""
    if not WEBSOCKET_AVAILABLE:
        print("websockets库未安装")
        return
    
    async def _send():
        async with websockets.connect(websocket_url) as ws:
            await ws.send(json.dumps({
                'type': 'metrics',
                'data': metrics_data
            }))
    
    asyncio.run(_send())


if __name__ == "__main__":
    server = RealtimeMonitorServer()
    server.run()
