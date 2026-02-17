#!/usr/bin/env python3
"""
AI行为改进 - 可视化报告生成器
生成HTML图表报告，便于直观查看改进趋势
"""

import json
import os
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Any
from collections import defaultdict

DATA_DIR = Path(__file__).parent / "monitoring_data"
DAILY_LOG_FILE = DATA_DIR / "daily_logs.json"
LOG_FILE = DATA_DIR / "error_logs.json"
REPORT_FILE = Path(__file__).parent / "ai_improvement_report.html"


def load_data():
    """加载监控数据"""
    daily_metrics = {}
    error_logs = []
    
    if DAILY_LOG_FILE.exists():
        with open(DAILY_LOG_FILE, 'r', encoding='utf-8') as f:
            daily_metrics = json.load(f)
    
    if LOG_FILE.exists():
        with open(LOG_FILE, 'r', encoding='utf-8') as f:
            error_logs = json.load(f)
    
    return daily_metrics, error_logs


def generate_html_report():
    """生成HTML可视化报告"""
    daily_metrics, error_logs = load_data()
    
    if not daily_metrics:
        return "<html><body><h1>暂无数据</h1></body></html>"
    
    # 准备数据
    dates = sorted(daily_metrics.keys())[-30:]  # 最近30天
    
    # 趋势数据
    interaction_data = []
    error_rate_data = []
    completion_rate_data = []
    
    for date in dates:
        m = daily_metrics[date]
        interaction_data.append(m.get('total_interactions', 0))
        error_rate_data.append(round(m.get('error_rate', 0), 2))
        completion_rate_data.append(round(m.get('completion_rate', 0), 2))
    
    # 错误类型分布
    error_types = defaultdict(int)
    for error in error_logs:
        if error['date'] in dates:
            error_types[error['error_type']] += 1
    
    error_type_labels = list(error_types.keys())
    error_type_data = list(error_types.values())
    
    # 严重度分布
    severity_counts = defaultdict(int)
    for error in error_logs:
        if error['date'] in dates:
            severity_counts[error['severity']] += 1
    
    # 计算统计
    total_interactions = sum(daily_metrics[d]['total_interactions'] for d in dates if d in daily_metrics)
    total_errors = sum(error_types.values())
    avg_error_rate = (total_errors / total_interactions * 100) if total_interactions > 0 else 0
    avg_completion = sum(completion_rate_data) / len(completion_rate_data) if completion_rate_data else 0
    
    html = f'''<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>AI行为改进报告</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            padding: 20px;
            min-height: 100vh;
        }}
        .container {{
            max-width: 1400px;
            margin: 0 auto;
        }}
        header {{
            text-align: center;
            color: white;
            margin-bottom: 30px;
            padding: 20px;
        }}
        header h1 {{
            font-size: 2.5em;
            margin-bottom: 10px;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.2);
        }}
        header p {{
            font-size: 1.1em;
            opacity: 0.9;
        }}
        .dashboard {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        .metric-card {{
            background: white;
            border-radius: 16px;
            padding: 25px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.1);
            transition: transform 0.3s;
        }}
        .metric-card:hover {{
            transform: translateY(-5px);
        }}
        .metric-card h3 {{
            color: #666;
            font-size: 0.9em;
            text-transform: uppercase;
            letter-spacing: 1px;
            margin-bottom: 10px;
        }}
        .metric-value {{
            font-size: 2.5em;
            font-weight: bold;
            color: #333;
        }}
        .metric-change {{
            font-size: 0.9em;
            margin-top: 5px;
        }}
        .positive {{
            color: #22c55e;
        }}
        .negative {{
            color: #ef4444;
        }}
        .chart-container {{
            background: white;
            border-radius: 16px;
            padding: 25px;
            margin-bottom: 20px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.1);
        }}
        .chart-container h2 {{
            color: #333;
            margin-bottom: 20px;
            padding-bottom: 10px;
            border-bottom: 2px solid #f0f0f0;
        }}
        .chart-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(500px, 1fr));
            gap: 20px;
        }}
        .chart-wrapper {{
            position: relative;
            height: 300px;
        }}
        .error-list {{
            background: white;
            border-radius: 16px;
            padding: 25px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.1);
        }}
        .error-list h2 {{
            color: #333;
            margin-bottom: 20px;
            padding-bottom: 10px;
            border-bottom: 2px solid #f0f0f0;
        }}
        .error-item {{
            padding: 15px;
            border-left: 4px solid #ef4444;
            background: #fef2f2;
            margin-bottom: 10px;
            border-radius: 0 8px 8px 0;
        }}
        .error-item .type {{
            font-weight: bold;
            color: #dc2626;
        }}
        .error-item .date {{
            font-size: 0.85em;
            color: #666;
            margin-top: 5px;
        }}
        .target-indicator {{
            display: inline-block;
            padding: 5px 12px;
            border-radius: 20px;
            font-size: 0.85em;
            font-weight: bold;
        }}
        .target-met {{
            background: #dcfce7;
            color: #166534;
        }}
        .target-missed {{
            background: #fee2e2;
            color: #991b1b;
        }}
        footer {{
            text-align: center;
            color: white;
            margin-top: 30px;
            padding: 20px;
            opacity: 0.8;
        }}
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>🤖 AI行为改进监控报告</h1>
            <p>生成时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} | 数据周期: 最近30天</p>
        </header>
        
        <div class="dashboard">
            <div class="metric-card">
                <h3>总交互次数</h3>
                <div class="metric-value">{total_interactions}</div>
                <div class="metric-change positive">📈 活跃监控中</div>
            </div>
            <div class="metric-card">
                <h3>错误率</h3>
                <div class="metric-value">{avg_error_rate:.2f}%</div>
                <div class="metric-change {'positive' if avg_error_rate < 5 else 'negative'}">
                    <span class="target-indicator {'target-met' if avg_error_rate < 5 else 'target-missed'}">
                        {'✓ 达标' if avg_error_rate < 5 else '✗ 未达标'}
                    </span>
                    (目标: &lt;5%)
                </div>
            </div>
            <div class="metric-card">
                <h3>任务完成率</h3>
                <div class="metric-value">{avg_completion:.1f}%</div>
                <div class="metric-change {'positive' if avg_completion >= 95 else 'negative'}">
                    <span class="target-indicator {'target-met' if avg_completion >= 95 else 'target-missed'}">
                        {'✓ 达标' if avg_completion >= 95 else '✗ 未达标'}
                    </span>
                    (目标: &gt;95%)
                </div>
            </div>
            <div class="metric-card">
                <h3>总错误数</h3>
                <div class="metric-value">{total_errors}</div>
                <div class="metric-change">📊 待分析改进</div>
            </div>
        </div>
        
        <div class="chart-grid">
            <div class="chart-container">
                <h2>📊 错误率趋势</h2>
                <div class="chart-wrapper">
                    <canvas id="errorRateChart"></canvas>
                </div>
            </div>
            <div class="chart-container">
                <h2>✅ 任务完成率趋势</h2>
                <div class="chart-wrapper">
                    <canvas id="completionRateChart"></canvas>
                </div>
            </div>
            <div class="chart-container">
                <h2>📈 每日交互量</h2>
                <div class="chart-wrapper">
                    <canvas id="interactionChart"></canvas>
                </div>
            </div>
            <div class="chart-container">
                <h2>🎯 错误类型分布</h2>
                <div class="chart-wrapper">
                    <canvas id="errorTypeChart"></canvas>
                </div>
            </div>
        </div>
        
        <div class="error-list">
            <h2>📝 近期错误记录</h2>
            {generate_error_list(error_logs[-10:]) if error_logs else '<p>暂无错误记录 ✓</p>'}
        </div>
        
        <footer>
            <p>AI行为改进监控系统 | 持续迭代，追求卓越</p>
        </footer>
    </div>
    
    <script>
        // 图表配置
        const dates = {dates};
        const errorRateData = {error_rate_data};
        const completionRateData = {completion_rate_data};
        const interactionData = {interaction_data};
        const errorTypeLabels = {error_type_labels};
        const errorTypeData = {error_type_data};
        
        // 错误率趋势图
        new Chart(document.getElementById('errorRateChart'), {{
            type: 'line',
            data: {{
                labels: dates,
                datasets: [{{
                    label: '错误率 (%)',
                    data: errorRateData,
                    borderColor: '#ef4444',
                    backgroundColor: 'rgba(239, 68, 68, 0.1)',
                    borderWidth: 2,
                    tension: 0.4,
                    fill: true
                }},
                {{
                    label: '目标线 (5%)',
                    data: dates.map(() => 5),
                    borderColor: '#22c55e',
                    borderWidth: 2,
                    borderDash: [5, 5],
                    pointRadius: 0
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: false,
                plugins: {{
                    legend: {{ display: true }}
                }},
                scales: {{
                    y: {{
                        beginAtZero: true,
                        max: 20
                    }}
                }}
            }}
        }});
        
        // 完成率趋势图
        new Chart(document.getElementById('completionRateChart'), {{
            type: 'line',
            data: {{
                labels: dates,
                datasets: [{{
                    label: '完成率 (%)',
                    data: completionRateData,
                    borderColor: '#22c55e',
                    backgroundColor: 'rgba(34, 197, 94, 0.1)',
                    borderWidth: 2,
                    tension: 0.4,
                    fill: true
                }},
                {{
                    label: '目标线 (95%)',
                    data: dates.map(() => 95),
                    borderColor: '#22c55e',
                    borderWidth: 2,
                    borderDash: [5, 5],
                    pointRadius: 0
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: false,
                plugins: {{
                    legend: {{ display: true }}
                }},
                scales: {{
                    y: {{
                        beginAtZero: true,
                        max: 100
                    }}
                }}
            }}
        }});
        
        // 交互量柱状图
        new Chart(document.getElementById('interactionChart'), {{
            type: 'bar',
            data: {{
                labels: dates,
                datasets: [{{
                    label: '交互次数',
                    data: interactionData,
                    backgroundColor: '#667eea',
                    borderRadius: 4
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: false,
                plugins: {{
                    legend: {{ display: false }}
                }},
                scales: {{
                    y: {{
                        beginAtZero: true
                    }}
                }}
            }}
        }});
        
        // 错误类型饼图
        new Chart(document.getElementById('errorTypeChart'), {{
            type: 'doughnut',
            data: {{
                labels: errorTypeLabels,
                datasets: [{{
                    data: errorTypeData,
                    backgroundColor: [
                        '#ef4444',
                        '#f97316',
                        '#eab308',
                        '#22c55e',
                        '#3b82f6'
                    ],
                    borderWidth: 0
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: false,
                plugins: {{
                    legend: {{
                        position: 'right'
                    }}
                }}
            }}
        }});
    </script>
</body>
</html>'''
    
    return html


def generate_error_list(error_logs: List[Dict]) -> str:
    """生成错误列表HTML"""
    html = ""
    for error in reversed(error_logs):
        html += f'''
        <div class="error-item">
            <div class="type">[{error['severity']}] {error['error_type']}</div>
            <div>{error['scenario'][:100]}...</div>
            <div class="date">{error['date']} | {error['timestamp'][:19]}</div>
        </div>
        '''
    return html


def main():
    """主函数"""
    print("正在生成可视化报告...")
    
    html = generate_html_report()
    
    with open(REPORT_FILE, 'w', encoding='utf-8') as f:
        f.write(html)
    
    print(f"✓ 报告已生成: {REPORT_FILE}")
    print(f"  请在浏览器中打开查看")
    
    # 尝试自动打开（macOS）
    if os.path.exists(REPORT_FILE):
        os.system(f'open "{REPORT_FILE}"')


if __name__ == "__main__":
    main()
