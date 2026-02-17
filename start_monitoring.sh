#!/bin/bash
# AI行为改进实时监控系统 - 启动脚本

cd "$(dirname "$0")"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 检查Python
if ! command -v python3 &> /dev/null; then
    echo -e "${RED}错误: 未找到 Python3${NC}"
    exit 1
fi

# 检查依赖
check_dependency() {
    if ! python3 -c "import $1" 2>/dev/null; then
        echo -e "${YELLOW}安装依赖: $1${NC}"
        pip3 install $1
    fi
}

echo -e "${BLUE}检查依赖...${NC}"
check_dependency websockets

echo ""
echo "=========================================="
echo "  🤖 AI行为改进实时监控系统"
echo "=========================================="
echo ""
echo "选择操作:"
echo ""
echo "1) 启动监控服务器"
echo "2) 启动演示客户端（模拟数据）"
echo "3) 打开实时仪表板"
echo "4) 查看监控数据（文本）"
echo "5) 启动完整环境（服务器+演示+仪表板）"
echo ""
echo "0) 退出"
echo ""
read -p "请输入选项 [0-5]: " choice

case $choice in
    1)
        echo -e "${GREEN}启动监控服务器...${NC}"
        echo "WebSocket地址: ws://localhost:8765"
        echo "按 Ctrl+C 停止"
        echo ""
        python3 ai_realtime_server.py
        ;;
    
    2)
        echo -e "${GREEN}启动演示客户端...${NC}"
        echo "正在模拟AI助手交互数据"
        echo "按 Ctrl+C 停止"
        echo ""
        python3 demo_client.py
        ;;
    
    3)
        echo -e "${GREEN}打开实时仪表板...${NC}"
        if command -v open &> /dev/null; then
            open realtime_dashboard.html
        elif command -v xdg-open &> /dev/null; then
            xdg-open realtime_dashboard.html
        else
            echo -e "${YELLOW}请手动在浏览器中打开: realtime_dashboard.html${NC}"
        fi
        ;;
    
    4)
        echo -e "${GREEN}查看监控数据...${NC}"
        python3 ai_monitor.py daily
        echo ""
        python3 ai_monitor.py kpi
        ;;
    
    5)
        echo -e "${GREEN}启动完整环境...${NC}"
        
        # 启动服务器（后台）
        echo "1. 启动监控服务器..."
        python3 ai_realtime_server.py &
        SERVER_PID=$!
        sleep 2
        
        # 打开仪表板
        echo "2. 打开实时仪表板..."
        if command -v open &> /dev/null; then
            open realtime_dashboard.html
        elif command -v xdg-open &> /dev/null; then
            xdg-open realtime_dashboard.html
        fi
        sleep 1
        
        # 启动演示客户端
        echo "3. 启动演示客户端..."
        echo ""
        python3 demo_client.py
        
        # 清理
        echo ""
        echo "清理进程..."
        kill $SERVER_PID 2>/dev/null
        echo -e "${GREEN}已停止所有服务${NC}"
        ;;
    
    0)
        echo "退出"
        exit 0
        ;;
    
    *)
        echo -e "${RED}无效选项${NC}"
        exit 1
        ;;
esac
