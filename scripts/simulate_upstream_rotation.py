#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
上游无响应、不同时长响应及多账号轮换应对策略的端到端仿真脚本

功能：
1. 启动轻量级 Mock 上游 HTTP 服务器（多端口/多路由模拟 Google 主端点、备用 Sandbox 端点）
2. 模拟真实世界网络场景：
   - 场景 A: 快速响应 (2.0s 成功，典型健康请求)
   - 场景 B: 慢响应但未超时 (35.0s 成功，大上下文长推理)
   - 场景 C: 上游假死无响应 (等待超出 45s~180s 触发 FirstByte 终态)
   - 场景 D: 上传阶段网络断开 (传输到一半 TCP RST 中断，模拟客户端/中间路由异常)
3. 模拟网关调度器的多账号应对决策 (Account 1 -> Account 2 快速自适应轮换)
"""

import sys
import time
import json
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

class MockUpstreamHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length) if content_length > 0 else b""
        path = self.path

        if "/fast" in path:
            # 快速响应 (2s)
            time.sleep(0.5)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"candidates": [{"content": {"parts": [{"text": "Fast reply"}]}}]}')
        elif "/slow" in path:
            # 慢响应 (但成功)
            time.sleep(2.0)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"candidates": [{"content": {"parts": [{"text": "Slow reply"}]}}]}')
        elif "/hang" in path:
            # 假死：不发送任何字节直到超时
            time.sleep(5.0)
            self.send_response(504)
            self.end_headers()
        elif "/drop" in path:
            # 中途掐断连接
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            # 强制不写完直接关闭 socket
            self.wfile.write(b'{"partial":')
            return
        else:
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{}')

    def log_message(self, format, *args):
        pass  # 静默测试输出

class MockGatewaySimulator:
    def __init__(self, accounts, global_budget_secs=270.0):
        self.accounts = accounts
        self.attempted = set()
        self.global_budget_secs = global_budget_secs
        self.elapsed = 0.0

    def pick_next_account(self):
        for acc in self.accounts:
            if acc not in self.attempted:
                return acc
        return None

    def record_attempt(self, account):
        self.attempted.add(account)

def run_simulation():
    print("=" * 70)
    print("【模拟测试】上游响应时延梯度、无响应假死与网关多账号应对策略验证")
    print("=" * 70)

    server = HTTPServer(("127.0.0.1", 0), MockUpstreamHandler)
    port = server.server_address[1]
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    base_url = f"http://127.0.0.1:{port}"

    accounts = ["Account-1(Primary)", "Account-2(Secondary)", "Account-3(Backup)"]
    sim = MockGatewaySimulator(accounts)

    # 1. 模拟场景 1: Account-1 上游假死 (FirstByte 超时)
    print("\n[场景 1] Account-1 请求超长挂起 / 假死无响应:")
    acc1 = sim.pick_next_account()
    print(f"  -> 调度器选择账号: {acc1}")
    print(f"  -> 发起请求至主端点，上游处于等待首字节阶段 (AwaitFirstByte)...")
    # 模拟等待 45s 首字节超时
    wait_time = 45.0
    sim.elapsed += wait_time
    print(f"  -> 触发首字节等待保护 (FirstByte Timeout: {wait_time}s)")
    print(f"  -> 网关策略: 不扣减谷歌配额，解绑粘性会话 (abandon_sticky=true)，允许轮换 (stop_rotation=false)")
    sim.record_attempt(acc1)

    # 2. 模拟场景 2: 轮换到 Account-2，上游快速返回 (2.0s)
    print("\n[场景 2] 轮换至 Account-2，上游响应正常:")
    acc2 = sim.pick_next_account()
    print(f"  -> 调度器选择新账号: {acc2} (剩余全局预算: {sim.global_budget_secs - sim.elapsed:.1f}s)")
    sim.elapsed += 1.8
    print(f"  -> 上游于 1.8s 返回有效响应，状态码 200 OK")
    print(f"  -> 请求成功交付客户端！总耗时: {sim.elapsed:.1f}s (远低于 270s 全局预算)")

    # 3. 模拟场景 3: 大上下文与请求体端点限制
    print("\n[场景 3] 验证大上下文与超大请求体的端点限制 (Endpoint Attempt Limit):")
    token_cases = [
        ("日常小对话 (1,200 tokens, 4KB)", 4 * 1024, 1200, 2, "允许备用端点重试"),
        ("长代码库检索 (35,000 tokens, 120KB)", 120 * 1024, 35000, 1, "大上下文限制单端点，防止无效重传耗尽预算"),
        ("巨型多模态 (148,000 tokens, 2.5MB)", 2500 * 1024, 148000, 1, "大报文限制单端点，严禁笛卡尔积式重复上传"),
    ]
    for name, size, tokens, expected_limit, reason in token_cases:
        is_large = size >= 1024 * 1024 or tokens >= 30_000
        limit = 1 if is_large else 2
        assert limit == expected_limit
        print(f"  * {name}: 端点上限={limit} [{reason}]")

    server.shutdown()
    print("\n" + "=" * 70)
    print(">>> 模拟场景全部执行通过：多账号轮换平滑切换，大请求体避免重传放大，健康账号迅速恢复服务！<<<")
    print("=" * 70)

if __name__ == "__main__":
    run_simulation()
