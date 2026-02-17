# AI行为改进实时监控系统 - 生产部署指南

> 部署生产级实时监控系统的完整指南

**📚 相关文档：** [改进指南](AI_IMPROVEMENT_GUIDE.md) | [监控指南](MONITORING_GUIDE.md) | [追踪表](ai-improvement-tracking.md)

---

## 🎯 系统概述

生产级实时监控系统包含三个核心组件：
1. **监控服务器** (`ai_realtime_server.py`) - WebSocket服务，接收和存储数据
2. **实时仪表板** (`realtime_dashboard.html`) - Web可视化界面
3. **数据客户端** - 集成到AI助手中，发送实时指标

## 📋 系统要求

### 硬件要求
- **最小配置**: 1核CPU, 512MB RAM, 1GB存储
- **推荐配置**: 2核CPU, 1GB RAM, 5GB存储
- **生产配置**: 4核CPU, 4GB RAM, 20GB SSD存储

### 软件依赖
```bash
Python >= 3.8
pip3
SQLite3 (通常已包含在Python中)
```

### Python依赖包
```bash
pip3 install websockets
```

## 🚀 快速部署

### 1. 基础部署（单服务器）

```bash
# 1. 进入项目目录
cd /path/to/bee

# 2. 安装依赖
pip3 install websockets

# 3. 启动监控服务器
python3 ai_realtime_server.py

# 4. 在浏览器中打开仪表板
open realtime_dashboard.html  # macOS
# 或
xdg-open realtime_dashboard.html  # Linux
```

### 2. 使用启动脚本

```bash
# 使用交互式启动脚本
./start_monitoring.sh

# 选择:
# 1) 启动监控服务器
# 2) 启动演示客户端
# 3) 打开实时仪表板
# 4) 查看监控数据（文本）
# 5) 启动完整环境（服务器+演示+仪表板）
```

## 🔧 生产环境配置

### 1. 配置报警阈值

编辑 `monitoring_data/alert_config.json`:

```json
{
  "error_rate_threshold": 5.0,
  "response_time_threshold": 30000,
  "consecutive_errors_threshold": 3,
  "alert_cooldown_minutes": 15,
  "enabled": true,
  "webhook_url": "https://your-webhook-url.com/alerts",
  "email_notifications": true
}
```

### 2. 配置WebSocket服务器

在代码中修改服务器地址:

```python
# ai_realtime_server.py
server = RealtimeMonitorServer(
    host="0.0.0.0",  # 监听所有接口
    port=8765        # 端口
)
```

### 3. 数据库优化

SQLite性能优化:

```sql
-- 连接数据库后执行
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA cache_size=10000;
PRAGMA temp_store=MEMORY;
```

## 🔌 集成到AI助手

### 方式1: 直接集成（推荐）

在AI助手的核心处理流程中添加监控代码:

```python
import asyncio
import websockets
import json
from datetime import datetime

class AIMonitorClient:
    def __init__(self, server_url="ws://localhost:8765"):
        self.server_url = server_url
        self.ws = None
    
    async def connect(self):
        self.ws = await websockets.connect(self.server_url)
    
    async def log_interaction(self, user_input, intent, tools_used, 
                              response_time, success, error_type=None):
        metrics = {
            "session_id": f"{datetime.now().strftime('%Y%m%d_%H%M%S')}",
            "timestamp": datetime.now().isoformat(),
            "user_input": user_input[:200],
            "intent": intent,
            "tools_used": tools_used,
            "response_time_ms": response_time,
            "success": success,
            "error_type": error_type
        }
        
        await self.ws.send(json.dumps({
            "type": "metrics",
            "data": metrics
        }))

# 在AI助手中使用
monitor = AIMonitorClient()
await monitor.connect()

# 每次交互后记录
await monitor.log_interaction(
    user_input="用户输入",
    intent="理解的意图",
    tools_used=[{"tool": "read", "params": {}}],
    response_time=1500,
    success=True
)
```

### 方式2: 通过中间件

创建一个监控中间件层:

```python
from ai_realtime_tracker import tracker

class AIMiddleware:
    async def process_request(self, request):
        with tracker.track_interaction(request.user_input) as t:
            # 记录意图
            intent = await self.understand_intent(request)
            t.log_intent(intent)
            
            # 执行工具
            result = await self.execute(intent)
            t.log_tool_use(result.tool, result.params)
            
            # 自动记录完成
            return result
```

## 🐳 Docker部署

### Dockerfile

```dockerfile
FROM python:3.11-slim

WORKDIR /app

# 安装依赖
RUN pip install websockets

# 复制文件
COPY ai_realtime_server.py .
COPY realtime_dashboard.html .
COPY demo_client.py .
COPY start_monitoring.sh .

# 创建数据目录
RUN mkdir -p monitoring_data

# 暴露端口
EXPOSE 8765

# 启动命令
CMD ["python3", "ai_realtime_server.py"]
```

### docker-compose.yml

```yaml
version: '3.8'

services:
  ai-monitor:
    build: .
    ports:
      - "8765:8765"
    volumes:
      - ./monitoring_data:/app/monitoring_data
    restart: unless-stopped
    environment:
      - PYTHONUNBUFFERED=1
  
  # 可选: Nginx反向代理
  nginx:
    image: nginx:alpine
    ports:
      - "80:80"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf
      - ./realtime_dashboard.html:/usr/share/nginx/html/index.html
    depends_on:
      - ai-monitor
```

### 部署命令

```bash
# 构建并启动
docker-compose up -d

# 查看日志
docker-compose logs -f ai-monitor

# 停止
docker-compose down
```

## ☸️ Kubernetes部署

### deployment.yaml

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ai-monitor
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ai-monitor
  template:
    metadata:
      labels:
        app: ai-monitor
    spec:
      containers:
      - name: ai-monitor
        image: your-registry/ai-monitor:latest
        ports:
        - containerPort: 8765
        volumeMounts:
        - name: data
          mountPath: /app/monitoring_data
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: ai-monitor-data
---
apiVersion: v1
kind: Service
metadata:
  name: ai-monitor
spec:
  selector:
    app: ai-monitor
  ports:
  - port: 8765
    targetPort: 8765
  type: LoadBalancer
```

## 📊 性能优化

### 1. 数据库优化

```python
# 定期清理旧数据
async def cleanup_old_data(days=30):
    """清理30天前的数据"""
    cutoff_date = (datetime.now() - timedelta(days=days)).strftime('%Y-%m-%d')
    
    with sqlite3.connect(DB_FILE) as conn:
        cursor = conn.cursor()
        cursor.execute("DELETE FROM sessions WHERE date < ?", (cutoff_date,))
        cursor.execute("DELETE FROM alerts WHERE created_at < datetime('now', '-30 days')")
        conn.commit()
```

### 2. WebSocket优化

```python
# 启用压缩
async with websockets.connect(
    server_url,
    compression=None  # 或选择合适的压缩算法
):
    pass
```

### 3. 批量写入

```python
# 批量插入提高性能
async def batch_insert(metrics_list):
    with sqlite3.connect(DB_FILE) as conn:
        cursor = conn.cursor()
        cursor.executemany('''
            INSERT INTO sessions (...)
            VALUES (?, ?, ?, ...)
        ''', metrics_list)
        conn.commit()
```

## 🔒 安全考虑

### 1. WebSocket认证

```python
async def handle_client(self, websocket, path):
    # 验证token
    token = await websocket.recv()
    if not self.validate_token(token):
        await websocket.close()
        return
    
    # 继续处理...
```

### 2. 数据加密

```python
import ssl

# 使用WSS (WebSocket Secure)
ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ssl_context.load_cert_chain('cert.pem', 'key.pem')

async with websockets.serve(
    handler,
    host,
    port,
    ssl=ssl_context
):
    pass
```

### 3. 访问控制

```python
# IP白名单
ALLOWED_IPS = ['127.0.0.1', '10.0.0.0/8']

async def handle_client(self, websocket, path):
    client_ip = websocket.remote_address[0]
    if not self.is_ip_allowed(client_ip):
        await websocket.close()
        return
```

## 📈 监控和日志

### 系统日志

```bash
# 使用systemd管理
sudo systemctl status ai-monitor
sudo journalctl -u ai-monitor -f
```

### 应用日志

```python
import logging

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('ai_monitor.log'),
        logging.StreamHandler()
    ]
)
```

### 健康检查

```python
async def health_check():
    """健康检查端点"""
    return {
        "status": "healthy",
        "connections": len(self.connected_clients),
        "database": self.check_db_connection(),
        "timestamp": datetime.now().isoformat()
    }
```

## 🔄 备份策略

### 自动备份脚本

```bash
#!/bin/bash
# backup.sh

BACKUP_DIR="/backups/ai-monitor"
DATE=$(date +%Y%m%d_%H%M%S)

# 备份SQLite数据库
sqlite3 monitoring_data/realtime_monitoring.db ".backup ${BACKUP_DIR}/backup_${DATE}.db"

# 保留最近30天的备份
find ${BACKUP_DIR} -name "backup_*.db" -mtime +30 -delete
```

### 定时任务

```bash
# 每天凌晨2点备份
0 2 * * * /path/to/backup.sh
```

## 🆘 故障排除

### 常见问题

**Q: WebSocket连接失败**
```bash
# 检查服务是否运行
lsof -i :8765

# 检查防火墙
sudo ufw allow 8765
```

**Q: 数据库锁定**
```bash
# 查看锁定状态
fuser monitoring_data/realtime_monitoring.db

# 重启服务
pkill -f ai_realtime_server
python3 ai_realtime_server.py
```

**Q: 内存不足**
```bash
# 限制Python内存使用
ulimit -v 1048576  # 1GB

# 或使用systemd限制
# 在service文件中添加:
# MemoryLimit=1G
```

## 📞 支持和维护

### 日常维护任务

- [ ] 每日检查报警
- [ ] 每周审查性能指标
- [ ] 每月备份验证
- [ ] 每季度更新依赖

### 升级流程

1. 备份现有数据
2. 停止服务
3. 更新代码
4. 测试新版本
5. 重新启动
6. 验证功能

---

**部署状态**: 就绪  
**最后更新**: 2026-02-17  
**版本**: v1.0.0
