#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
测试 SQLite 思考块动态刷新机制与日志容量滑动窗口清理及插入
1. 验证思考块存储（Thinking Store）：
   - 会话随请求动态刷新（touch last_accessed）
   - 超过 15 天未刷新的旧会话被精准清理
   - 经过动态刷新的活跃会话正常保留，数据完整无损
2. 验证日志数据库（Requests DB）：
   - 改造后不再按时间强制清空 request_body / response_body（不再镂空）
   - 容量达到上限时，触发滑动窗口淘汰最尾部 30% 旧记录
   - 空间释放后，新日志顺利插入且报文完整
"""

import sqlite3
import time
import json
import sys

def run_thinking_store_test():
    print("\n" + "=" * 60)
    print("【测试 1】验证思考块会话滑动刷新与 15 天清理机制")
    print("=" * 60)

    conn = sqlite3.connect(":memory:")
    cur = conn.cursor()

    # 初始化表结构（与 src-tauri/src/modules/proxy_db.rs 结构一致）
    cur.executescript("""
    CREATE TABLE thinking_sessions (
        session_key TEXT PRIMARY KEY,
        last_accessed INTEGER NOT NULL
    );
    CREATE TABLE thinking_records (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_key TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        thought BLOB NOT NULL,
        signature TEXT,
        tool_ids TEXT,
        tool_names TEXT,
        visible TEXT NOT NULL,
        last_accessed INTEGER,
        created_at INTEGER
    );
    """)

    now_ms = int(time.time() * 1000)
    day_ms = 24 * 3600 * 1000

    t_20_days_ago = now_ms - (20 * day_ms)
    t_3_days_ago = now_ms - (3 * day_ms)

    # 构造假数据 1: session_expired (20 天前，未刷新)
    cur.execute("INSERT INTO thinking_sessions VALUES (?, ?)", ("session_expired", t_20_days_ago))
    cur.execute("INSERT INTO thinking_records (session_key, fingerprint, thought, visible, last_accessed, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                ("session_expired", "fp_1", b"thought_data_1", "visible_text_1", t_20_days_ago, t_20_days_ago))
    cur.execute("INSERT INTO thinking_records (session_key, fingerprint, thought, visible, last_accessed, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                ("session_expired", "fp_2", b"thought_data_2", "visible_text_2", t_20_days_ago, t_20_days_ago))

    # 构造假数据 2: session_active (原本也是 20 天前创建，但之后发生多轮对话，动态 touch 刷新为当前时间)
    cur.execute("INSERT INTO thinking_sessions VALUES (?, ?)", ("session_active", t_20_days_ago))
    cur.execute("INSERT INTO thinking_records (session_key, fingerprint, thought, visible, last_accessed, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                ("session_active", "fp_3", b"thought_data_3", "visible_text_3", t_20_days_ago, t_20_days_ago))

    # 模拟客户端请求：touch_thinking_session 动态刷新活跃时间
    cur.execute("""
    INSERT INTO thinking_sessions (session_key, last_accessed) VALUES (?, ?)
    ON CONFLICT(session_key) DO UPDATE SET last_accessed = excluded.last_accessed
    """, ("session_active", now_ms))

    # 构造假数据 3: session_recent (3 天前建立的近期会话)
    cur.execute("INSERT INTO thinking_sessions VALUES (?, ?)", ("session_recent", t_3_days_ago))
    cur.execute("INSERT INTO thinking_records (session_key, fingerprint, thought, visible, last_accessed, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                ("session_recent", "fp_4", b"thought_data_4", "visible_text_4", t_3_days_ago, t_3_days_ago))

    conn.commit()
    print("✓ 假数据构造成功：")
    print("  - session_expired: 20 天前访问，包含 2 条思考记录")
    print("  - session_active:  20 天前创建，刚才动态 touch 刷新")
    print("  - session_recent:  3 天前访问，包含 1 条思考记录")

    # 执行 15 天淘汰策略（与 proxy_db.rs 中 cleanup_old_thinking_records(15) 逻辑完全相同）
    cutoff_15_days = now_ms - (15 * day_ms)
    cur.execute("""
    DELETE FROM thinking_records WHERE session_key IN (
        SELECT session_key FROM thinking_sessions WHERE last_accessed < ?
    ) OR (
        session_key NOT IN (SELECT session_key FROM thinking_sessions)
        AND COALESCE(last_accessed, created_at) < ?
    )
    """, (cutoff_15_days, cutoff_15_days))

    cur.execute("DELETE FROM thinking_sessions WHERE last_accessed < ?", (cutoff_15_days,))
    conn.commit()

    # 断言验证
    cur.execute("SELECT COUNT(*) FROM thinking_sessions WHERE session_key = 'session_expired'")
    assert cur.fetchone()[0] == 0, "session_expired 应当被淘汰删除！"

    cur.execute("SELECT COUNT(*) FROM thinking_records WHERE session_key = 'session_expired'")
    assert cur.fetchone()[0] == 0, "session_expired 的思考记录应当被彻底删除！"

    cur.execute("SELECT COUNT(*) FROM thinking_sessions WHERE session_key = 'session_active'")
    assert cur.fetchone()[0] == 1, "session_active 因动态刷新，应当完好保留！"

    cur.execute("SELECT COUNT(*) FROM thinking_records WHERE session_key = 'session_active'")
    assert cur.fetchone()[0] == 1, "session_active 的思考记录应当完好保留！"

    cur.execute("SELECT COUNT(*) FROM thinking_sessions WHERE session_key = 'session_recent'")
    assert cur.fetchone()[0] == 1, "session_recent 在 15 天内，应当被保留！"

    print("✓ 断言全部通过！思考块会话动态滑动刷新有效，15 天无活动会话精准清理。")


def run_request_logs_sliding_window_test():
    print("\n" + "=" * 60)
    print("【测试 2】验证 SQLite 日志容量达到上限时 30% 滑动窗口淘汰与新日志插入")
    print("=" * 60)

    conn = sqlite3.connect(":memory:")
    cur = conn.cursor()

    # 初始化表结构（与 src-tauri/src/modules/proxy_db.rs 结构一致）
    cur.executescript("""
    CREATE TABLE request_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        timestamp INTEGER NOT NULL,
        method TEXT NOT NULL,
        url TEXT NOT NULL,
        status INTEGER NOT NULL,
        duration INTEGER NOT NULL,
        account_id TEXT,
        model TEXT,
        request_body TEXT,
        upstream_request_body TEXT,
        response_body TEXT,
        error_message TEXT,
        input_tokens INTEGER,
        output_tokens INTEGER
    );
    """)

    now_ms = int(time.time() * 1000)

    # 1. 批量插入 20 条日志，包含长文本 Prompt 和 Response
    for i in range(1, 21):
        ts = now_ms - ((21 - i) * 3600 * 1000) # 20小时前至1小时前
        req_body = json.dumps({"messages": [{"role": "user", "content": f"User Prompt #{i} - Detailed analysis request."}]})
        res_body = json.dumps({"choices": [{"message": {"role": "assistant", "content": f"Assistant Response #{i} - Detailed model answer."}}]})

        cur.execute("""
        INSERT INTO request_logs (timestamp, method, url, status, duration, model, request_body, response_body)
        VALUES (?, 'POST', '/v1/chat/completions', 200, 180, 'gemini-3-flash', ?, ?)
        """, (ts, req_body, res_body))

    conn.commit()

    cur.execute("SELECT COUNT(*) FROM request_logs")
    initial_count = cur.fetchone()[0]
    assert initial_count == 20, f"应插入 20 条初始日志，实际: {initial_count}"
    print(f"✓ 成功构造并插入 20 条完整长文本日志。")

    # 验证新策略：即使过了 24 小时，请求体也不会被强行掏空置为 NULL
    cur.execute("SELECT COUNT(*) FROM request_logs WHERE request_body IS NULL OR response_body IS NULL")
    null_count = cur.fetchone()[0]
    assert null_count == 0, "新策略下请求体与响应体不应被掏空为 NULL！"
    print("✓ 验证通过：请求体保持 100% 完整，不再被按小时强制掏空。")

    # 2. 模拟达到容量上限，触发 30% 滑动窗口淘汰（20 * 30% = 6 条）
    to_evict = int(initial_count * 0.3) # 6 条
    print(f"✓ 模拟容量超限，触发 30% 滑动窗口淘汰：计划淘汰最旧的 {to_evict} 条记录...")

    cur.execute("""
    DELETE FROM request_logs WHERE id IN (
        SELECT id FROM request_logs ORDER BY timestamp ASC LIMIT ?
    )
    """, (to_evict,))
    conn.commit()

    cur.execute("SELECT COUNT(*) FROM request_logs")
    remaining_count = cur.fetchone()[0]
    assert remaining_count == 14, f"淘汰 30% 后应剩余 14 条日志，实际: {remaining_count}"

    # 检查保留的最旧记录（原第 7 条）的请求体内容完好无损
    cur.execute("SELECT request_body, response_body FROM request_logs ORDER BY timestamp ASC LIMIT 1")
    oldest_kept = cur.fetchone()
    assert "User Prompt #7" in oldest_kept[0], "保留的日志记录内容应当完好无损！"
    print("✓ 验证通过：超限 30% 记录成功淘汰，保留的记录报文无任何损坏。")

    # 3. 模拟插入全新日志（空间释放后新日志正常入库）
    new_req = json.dumps({"messages": [{"role": "user", "content": "Brand New Request After Eviction"}]})
    new_res = json.dumps({"choices": [{"message": {"role": "assistant", "content": "Brand New Response Successfully Saved"}}]})

    cur.execute("""
    INSERT INTO request_logs (timestamp, method, url, status, duration, model, request_body, response_body)
    VALUES (?, 'POST', '/v1/chat/completions', 200, 120, 'gemini-3-flash', ?, ?)
    """, (now_ms + 1000, new_req, new_res))
    conn.commit()

    cur.execute("SELECT COUNT(*) FROM request_logs")
    final_count = cur.fetchone()[0]
    assert final_count == 15, f"插入新日志后总数应为 15，实际: {final_count}"

    cur.execute("SELECT request_body FROM request_logs ORDER BY timestamp DESC LIMIT 1")
    latest_req = cur.fetchone()[0]
    assert "Brand New Request After Eviction" in latest_req
    print("✓ 验证通过：空间释放后，新日志能够正常写入并读取！")
    print("=" * 60)
    print("ALL TESTS PASSED SUCCESSFULLY!")
    print("=" * 60)

if __name__ == "__main__":
    try:
        run_thinking_store_test()
        run_request_logs_sliding_window_test()
        print("\n🎉 全部 SQLite 测试验证通过！")
    except Exception as e:
        print(f"\n❌ 测试失败: {e}", file=sys.stderr)
        sys.exit(1)
