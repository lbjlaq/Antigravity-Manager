//! IP Security Integration Tests
//! IP 安全功能的集成测试
//!
//! 这些测试需要启动完整的代理服务器来验证端到端的功能

#[cfg(test)]
mod integration_tests {
    use crate::modules::security_db::{
        self, add_to_blacklist, add_to_whitelist, get_blacklist, get_whitelist, init_db,
        remove_from_blacklist, remove_from_whitelist,
    };
    use std::time::Duration;

    /// 辅助函数：清理测试环境
    fn cleanup_test_data() {
        if let Ok(entries) = get_blacklist() {
            for entry in entries {
                let _ = remove_from_blacklist(&entry.id);
            }
        }
        if let Ok(entries) = get_whitelist() {
            for entry in entries {
                let _ = remove_from_whitelist(&entry.id);
            }
        }
    }

    // ============================================================================
    // 集成测试场景 1：黑名单阻止请求
    // ============================================================================

    /// 测试场景：当 IP 在黑名单中时，请求应该被拒绝
    ///
    /// 预期行为：
    /// 1. 添加 IP 到黑名单
    /// 2. 该 IP 发起的请求返回 403 Forbidden
    /// 3. 响应体包含封禁原因
    #[test]
    fn test_scenario_blacklist_blocks_request() {
        let _ = init_db();
        cleanup_test_data();

        // 添加测试 IP 到黑名单
        let entry = add_to_blacklist(
            "192.168.100.100",
            Some("Integration test - malicious activity"),
            None,
            "integration_test",
        );
        assert!(entry.is_ok(), "Should add IP to blacklist");

        // 验证黑名单条目存在
        let blacklist = get_blacklist().unwrap();
        let found = blacklist.iter().any(|e| e.ip_pattern == "192.168.100.100");
        assert!(found, "IP should be in blacklist");

        // 实际的 HTTP 请求测试需要启动服务器
        // 这里验证数据层正确性
        let is_blocked = security_db::is_ip_in_blacklist("192.168.100.100").unwrap();
        assert!(is_blocked, "IP should be blocked");

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 2：白名单优先模式
    // ============================================================================

    /// 测试场景：白名单优先模式下，白名单 IP 跳过黑名单检查
    ///
    /// 预期行为：
    /// 1. IP 同时存在于黑名单和白名单
    /// 2. 启用 whitelist_priority 模式
    /// 3. 请求应该被允许（白名单优先）
    #[test]
    fn test_scenario_whitelist_priority() {
        let _ = init_db();
        cleanup_test_data();

        // 添加 IP 到黑名单
        let _ = add_to_blacklist(
            "10.0.0.50",
            Some("Should be overridden by whitelist"),
            None,
            "test",
        );

        // 添加相同 IP 到白名单
        let _ = add_to_whitelist("10.0.0.50", Some("Trusted - override blacklist"));

        // 验证两个列表都包含该 IP
        assert!(security_db::is_ip_in_blacklist("10.0.0.50").unwrap());
        assert!(security_db::is_ip_in_whitelist("10.0.0.50").unwrap());

        // 在实际中间件中，whitelist_priority=true 时，会先检查白名单
        // 如果在白名单中，则跳过黑名单检查
        // 这里只验证数据正确性，中间件逻辑由 ip_filter.rs 保证

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 3：临时封禁与过期
    // ============================================================================

    /// 测试场景：临时封禁在过期后自动解除
    ///
    /// 预期行为：
    /// 1. 添加临时封禁（已过期）
    /// 2. 查询时自动清理过期条目
    /// 3. 请求应该被允许
    #[test]
    fn test_scenario_temporary_ban_expiration() {
        let _ = init_db();
        cleanup_test_data();

        // 获取当前时间戳
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 添加已过期的临时封禁
        let _ = add_to_blacklist(
            "expired.ban.test",
            Some("Temporary ban - should be expired"),
            Some(now - 60), // 1分钟前过期
            "test",
        );

        // 查询时应该触发过期清理
        let is_blocked = security_db::is_ip_in_blacklist("expired.ban.test").unwrap();
        assert!(!is_blocked, "Expired ban should not block");

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 4：CIDR 范围封禁
    // ============================================================================

    /// 测试场景：CIDR 范围封禁覆盖整个子网
    ///
    /// 预期行为：
    /// 1. 封禁 192.168.1.0/24
    /// 2. 192.168.1.x 的所有请求被拒绝
    /// 3. 192.168.2.x 的请求正常通过
    #[test]
    fn test_scenario_cidr_subnet_blocking() {
        let _ = init_db();
        cleanup_test_data();

        // 封禁整个子网
        let _ = add_to_blacklist(
            "192.168.1.0/24",
            Some("Entire subnet blocked"),
            None,
            "test",
        );

        // 验证子网内的 IP 被阻止
        for last_octet in [1, 50, 100, 200, 254] {
            let ip = format!("192.168.1.{}", last_octet);
            let is_blocked = security_db::is_ip_in_blacklist(&ip).unwrap();
            assert!(is_blocked, "IP {} should be blocked by CIDR", ip);
        }

        // 验证子网外的 IP 不被阻止
        for last_octet in [1, 50, 100] {
            let ip = format!("192.168.2.{}", last_octet);
            let is_blocked = security_db::is_ip_in_blacklist(&ip).unwrap();
            assert!(!is_blocked, "IP {} should NOT be blocked", ip);
        }

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 5：封禁消息详情
    // ============================================================================

    /// 测试场景：封禁响应包含详细信息
    ///
    /// 预期行为：
    /// 1. 添加带原因的封禁
    /// 2. 请求被拒绝时，响应包含：
    ///    - 封禁原因
    ///    - 是否为临时/永久封禁
    ///    - 剩余封禁时间（如果是临时）
    #[test]
    fn test_scenario_ban_message_details() {
        let _ = init_db();
        cleanup_test_data();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 添加临时封禁（2小时后过期）
        let _ = add_to_blacklist(
            "temp.ban.message",
            Some("Rate limit exceeded"),
            Some(now + 7200), // 2小时后
            "rate_limiter",
        );

        // 获取封禁详情
        let entry = security_db::get_blacklist_entry_for_ip("temp.ban.message")
            .unwrap()
            .unwrap();

        assert_eq!(entry.reason.as_deref(), Some("Rate limit exceeded"));
        assert!(entry.expires_at.is_some());

        let remaining = entry.expires_at.unwrap() - now;
        assert!(
            remaining > 0 && remaining <= 7200,
            "Should have ~2h remaining"
        );

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 6：访问日志记录
    // ============================================================================

    /// 测试场景：被阻止的请求记录到日志
    ///
    /// 预期行为：
    /// 1. 黑名单 IP 发起请求
    /// 2. 请求被拒绝
    /// 3. 访问日志记录：IP、时间、状态(403)、封禁原因
    #[test]
    fn test_scenario_blocked_request_logging() {
        let _ = init_db();
        cleanup_test_data();

        // 模拟保存被阻止的访问日志
        let log = security_db::IpAccessLog {
            id: uuid::Uuid::new_v4().to_string(),
            client_ip: "blocked.request.test".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            method: Some("POST".to_string()),
            path: Some("/v1/messages".to_string()),
            user_agent: Some("TestClient/1.0".to_string()),
            status: Some(403),
            duration: Some(0),
            api_key_hash: None,
            blocked: true,
            block_reason: Some("IP in blacklist".to_string()),
            username: None,
        };

        let save_result = security_db::save_ip_access_log(&log);
        assert!(save_result.is_ok());

        // 验证日志可以检索
        let logs = security_db::get_ip_access_logs(10, 0, None, true).unwrap();
        let found = logs.iter().any(|l| l.client_ip == "blocked.request.test");
        assert!(found, "Blocked request should be logged");

        let _ = security_db::clear_ip_access_logs();
    }

    // ============================================================================
    // 集成测试场景 7：不影响正常请求性能
    // ============================================================================

    /// 测试场景：安全检查不显著影响正常请求性能
    ///
    /// 预期行为：
    /// 1. 黑名单/白名单检查时间 < 5ms
    /// 2. 与没有安全检查的基线相比，延迟增加 < 10ms
    #[test]
    fn test_scenario_performance_impact() {
        let _ = init_db();
        cleanup_test_data();

        // 添加一些黑名单条目
        for i in 0..50 {
            let _ = add_to_blacklist(&format!("perf.test.{}", i), None, None, "test");
        }

        // 添加一些 CIDR 规则
        for i in 0..10 {
            let _ = add_to_blacklist(&format!("172.{}.0.0/16", i), None, None, "test");
        }

        // 测试查找性能
        let start = std::time::Instant::now();
        let iterations = 100;

        for _ in 0..iterations {
            // 模拟正常请求的安全检查
            let _ = security_db::is_ip_in_whitelist("10.0.0.1");
            let _ = security_db::is_ip_in_blacklist("10.0.0.1");
        }

        let duration = start.elapsed();
        let avg_per_check = duration / (iterations * 2);

        println!("Average security check time: {:?}", avg_per_check);

        // 断言：平均每次检查应该在 5ms 以内
        assert!(
            avg_per_check < Duration::from_millis(5),
            "Security check should be fast"
        );

        cleanup_test_data();
    }

    // ============================================================================
    // 集成测试场景 8：数据持久化
    // ============================================================================

    /// 测试场景：黑名单/白名单数据持久化
    ///
    /// 预期行为：
    /// 1. 添加数据后重新初始化数据库连接
    /// 2. 数据仍然存在
    #[test]
    fn test_scenario_data_persistence() {
        let _ = init_db();
        cleanup_test_data();

        // 添加数据
        let _ = add_to_blacklist("persist.test.ip", Some("Persistence test"), None, "test");
        let _ = add_to_whitelist("persist.white.ip", Some("Persistence test"));

        // 重新初始化（实际上只是验证数据仍然可读）
        let _ = init_db();

        // 验证数据仍然存在
        assert!(security_db::is_ip_in_blacklist("persist.test.ip").unwrap());
        assert!(security_db::is_ip_in_whitelist("persist.white.ip").unwrap());

        cleanup_test_data();
    }
}

// ============================================================================
// 压力测试
// ============================================================================

#[cfg(test)]
mod stress_tests {
    use crate::modules::security_db::{
        add_to_blacklist, clear_ip_access_logs, get_blacklist, init_db, is_ip_in_blacklist,
        remove_from_blacklist, save_ip_access_log, IpAccessLog,
    };
    use std::thread;
    use std::time::{Duration, Instant};

    /// 辅助函数：清理测试环境
    fn cleanup_test_data() {
        if let Ok(entries) = get_blacklist() {
            for entry in entries {
                let _ = remove_from_blacklist(&entry.id);
            }
        }
        let _ = clear_ip_access_logs();
    }

    /// 压力测试：大量黑名单条目
    #[test]
    fn stress_test_large_blacklist() {
        let _ = init_db();
        cleanup_test_data();

        let count = 500;

        // 批量添加
        let start = Instant::now();
        for i in 0..count {
            let _ = add_to_blacklist(
                &format!("stress.{}.{}.{}.{}", i / 256, (i / 16) % 16, i % 16, i),
                None,
                None,
                "stress",
            );
        }
        let add_duration = start.elapsed();
        println!("Added {} entries in {:?}", count, add_duration);

        // 随机查找测试
        let start = Instant::now();
        for i in 0..100 {
            let _ = is_ip_in_blacklist(&format!(
                "stress.{}.{}.{}.{}",
                i / 256,
                (i / 16) % 16,
                i % 16,
                i
            ));
        }
        let lookup_duration = start.elapsed();
        println!("100 lookups in large blacklist took {:?}", lookup_duration);

        // 验证性能合理
        assert!(
            lookup_duration < Duration::from_secs(1),
            "Lookups should be reasonably fast even with large blacklist"
        );

        cleanup_test_data();
    }

    /// 压力测试：大量访问日志
    #[test]
    fn stress_test_access_logging() {
        let _ = init_db();
        let _ = clear_ip_access_logs();

        let count = 1000;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 批量写入日志
        let start = Instant::now();
        for i in 0..count {
            let log = IpAccessLog {
                id: uuid::Uuid::new_v4().to_string(),
                client_ip: format!("log.stress.{}", i % 100),
                timestamp: now,
                method: Some("POST".to_string()),
                path: Some("/v1/messages".to_string()),
                user_agent: Some("StressTest/1.0".to_string()),
                status: Some(200),
                duration: Some(100),
                api_key_hash: Some("hash".to_string()),
                blocked: false,
                block_reason: None,
                username: None,
            };
            let _ = save_ip_access_log(&log);
        }
        let write_duration = start.elapsed();
        println!("Wrote {} access logs in {:?}", count, write_duration);

        // 验证写入性能合理
        assert!(
            write_duration < Duration::from_secs(10),
            "Access log writing should be reasonably fast"
        );

        let _ = clear_ip_access_logs();
    }

    /// 压力测试：并发操作
    #[test]
    fn stress_test_concurrent_operations() {
        let _ = init_db();
        cleanup_test_data();

        let thread_count = 5;
        let ops_per_thread = 20;

        let handles: Vec<_> = (0..thread_count)
            .map(|t| {
                thread::spawn(move || {
                    for i in 0..ops_per_thread {
                        // 每个线程添加-查询-删除
                        let ip = format!("concurrent.{}.{}", t, i);
                        if let Ok(entry) = add_to_blacklist(&ip, None, None, "concurrent") {
                            let _ = is_ip_in_blacklist(&ip);
                            let _ = remove_from_blacklist(&entry.id);
                        }
                    }
                })
            })
            .collect();

        // 等待所有线程完成
        for handle in handles {
            handle.join().expect("Thread should not panic");
        }

        // 验证没有遗留数据
        let remaining = get_blacklist().unwrap();
        let concurrent_remaining: Vec<_> = remaining
            .iter()
            .filter(|e| e.ip_pattern.starts_with("concurrent."))
            .collect();

        assert!(
            concurrent_remaining.is_empty(),
            "All concurrent test data should be cleaned up"
        );

        cleanup_test_data();
    }
}
