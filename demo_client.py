#!/usr/bin/env python3
"""
AI行为改进 - 演示客户端
模拟AI助手向监控系统发送实时数据
"""

import asyncio
import json
import random
import time
from datetime import datetime

try:
    import websockets
except ImportError:
    print("请先安装websockets: pip install websockets")
    exit(1)


class DemoAIClient:
    """模拟AI助手客户端"""
    
    def __init__(self, server_url="ws://localhost:8765"):
        self.server_url = server_url
        self.session_count = 0
        self.error_types = ["意图误解", "工具误用", "路径错误", "输出不当", None, None, None]
        self.tools = ["read", "edit", "bash", "grep", "lsp", "glob"]
    
    async def run(self):
        """运行演示"""
        print(f"🔌 连接到监控服务器: {self.server_url}")
        
        try:
            async with websockets.connect(self.server_url) as ws:
                print("✅ 已连接")
                print("🎬 开始模拟AI助手交互...")
                print("按 Ctrl+C 停止\n")
                
                while True:
                    await self.simulate_interaction(ws)
                    await asyncio.sleep(random.uniform(1, 3))  # 1-3秒间隔
        
        except websockets.exceptions.ConnectionRefused:
            print("❌ 无法连接到服务器")
            print("请先启动监控服务器: python3 ai_realtime_server.py")
        except KeyboardInterrupt:
            print(f"\n\n✅ 演示结束")
            print(f"📊 共模拟 {self.session_count} 次交互")
    
    async def simulate_interaction(self, ws):
        """模拟一次交互"""
        self.session_count += 1
        
        # 模拟用户输入
        user_inputs = [
            "帮我查看这个文件",
            "计算 2+2",
            "搜索所有包含TODO的文件",
            "修改配置文件",
            "继续",
            "生成一个Python脚本",
            "怎样使用Docker部署",
            "修复这个bug",
            "查看项目结构",
            "优化这段代码"
        ]
        
        user_input = random.choice(user_inputs)
        
        # 模拟意图理解
        intents = {
            "帮我查看这个文件": "文件查看请求",
            "计算 2+2": "数学计算请求",
            "搜索所有包含TODO的文件": "代码搜索请求",
            "修改配置文件": "文件修改请求",
            "继续": "上下文延续请求",
            "生成一个Python脚本": "代码生成请求",
            "怎样使用Docker部署": "指导咨询请求",
            "修复这个bug": "错误修复请求",
            "查看项目结构": "项目探索请求",
            "优化这段代码": "代码优化请求"
        }
        
        intent = intents.get(user_input, "未知请求")
        
        # 模拟工具使用
        num_tools = random.randint(1, 3)
        tools_used = []
        for i in range(num_tools):
            tools_used.append({
                "tool": random.choice(self.tools),
                "params": {"file": "example.txt"},
                "timestamp": datetime.now().isoformat()
            })
        
        # 模拟响应时间 (100ms - 5000ms)
        response_time = random.randint(100, 5000)
        
        # 模拟成功率 (85%成功率)
        success = random.random() > 0.15
        
        # 模拟错误
        error_type = None
        error_severity = None
        if not success:
            error_type = random.choice(self.error_types[:-3])  # 偏向有错误
            error_severity = random.choice(["低", "中", "高"])
        
        # 构建指标数据
        metrics = {
            "session_id": f"demo_{self.session_count}_{int(time.time())}",
            "timestamp": datetime.now().isoformat(),
            "user_input": user_input,
            "intent": intent,
            "tools_used": tools_used,
            "response_time_ms": response_time,
            "success": success,
            "error_type": error_type,
            "error_severity": error_severity
        }
        
        # 发送给监控服务器
        await ws.send(json.dumps({
            "type": "metrics",
            "data": metrics
        }, ensure_ascii=False))
        
        # 打印信息
        status = "✅" if success else "❌"
        print(f"{status} [{self.session_count:3d}] {user_input[:30]:30s} | "
              f"{response_time:4d}ms | {len(tools_used)} tools | "
              f"{error_type if error_type else 'OK'}")


async def main():
    """主函数"""
    import sys
    
    server_url = sys.argv[1] if len(sys.argv) > 1 else "ws://localhost:8765"
    
    print("="*60)
    print("🤖 AI行为改进 - 演示客户端")
    print("="*60)
    print()
    
    client = DemoAIClient(server_url)
    await client.run()


if __name__ == "__main__":
    asyncio.run(main())
