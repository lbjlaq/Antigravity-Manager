# 三层渐进式上下文压缩测试方案 (v3.3.50)

## 测试目标

验证三层渐进式上下文压缩功能的正确性、稳定性和成本优化效果。

## 前置准备

1. **启动应用**：
   ```bash
   cd /Users/lbjlaq/Desktop/xin
   npm run tauri dev
   ```

2. **启用调试日志**：
   ```bash
   export RUST_LOG=debug
   ```

3. **准备测试账号**：
   - 至少 1 个 Google 账号（用于 Gemini API）
   - 确保账号有足够配额

## 测试场景

### 场景 1：Layer 1 工具消息裁剪 (60% 压力)

**目标**：验证工具消息智能裁剪功能

**步骤**：
1. 使用 Claude Code CLI 或 Cherry Studio
2. 发起一个需要多次工具调用的任务（如代码搜索、文件读取）
3. 持续对话直到触发 60% 上下文压力

**预期结果**：
- 日志中出现 `[Layer-1] Tool trimming triggered`
- 保留最近 5 轮工具交互
- 删除更早的工具消息
- **无 400 错误**
- **响应速度正常**

**验证命令**：
```bash
# 查看日志
tail -f ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep "Layer-1"
```

---

### 场景 2：Layer 2 Thinking 压缩 (75% 压力)

**目标**：验证 Thinking 内容压缩 + 签名保留

**步骤**：
1. 使用 Claude 4.5 Opus/Sonnet Thinking 模型
2. 发起复杂推理任务（如代码重构、算法设计）
3. 持续对话直到触发 75% 上下文压力

**预期结果**：
- 日志中出现 `[Layer-2] Thinking compression triggered`
- Thinking 块文本被压缩为 "..."
- **`signature` 字段完整保留**
- 最近 4 条消息不被压缩
- **无 400 签名错误**

**验证命令**：
```bash
# 查看签名保留情况
tail -f ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep -E "(Layer-2|signature)"
```

---

### 场景 3：Layer 3 Fork 会话 + XML 摘要 (90% 压力)

**目标**：验证 XML 摘要生成和会话 Fork

**步骤**：
1. 使用任意模型进行超长对话
2. 持续对话直到触发 90% 上下文压力

**预期结果**：
- 日志中出现 `[Layer-3] Critical context pressure`
- 调用 `gemini-2.5-flash-lite` 生成 XML 摘要
- 创建新的消息序列：`[User: XML摘要] + [Assistant: 确认] + [用户最新消息]`
- **压缩率 86-97%**
- **无 Prompt Cache 破坏**
- **签名链完整**

**验证命令**：
```bash
# 查看 Layer 3 触发和摘要生成
tail -f ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep -E "(Layer-3|XML summary|Fork)"
```

---

### 场景 4：渐进式触发测试

**目标**：验证三层压缩的渐进式触发机制

**步骤**：
1. 从空对话开始
2. 持续对话，观察压缩层级的触发顺序

**预期结果**：
- 触发顺序：Layer 1 (60%) → Layer 2 (75%) → Layer 3 (90%)
- 每次压缩后重新估算 Token 用量
- 日志中清晰记录每层的触发和效果

**验证命令**：
```bash
# 查看所有层级的触发
tail -f ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep -E "Layer-[123]"
```

---

### 场景 5：错误处理测试

**目标**：验证 Layer 3 失败时的容错机制

**步骤**：
1. 临时禁用 Gemini 账号或网络
2. 触发 Layer 3 压缩

**预期结果**：
- Layer 3 失败时返回 `BAD_REQUEST` 错误
- 错误消息友好：`Context too long and automatic compression failed`
- 提示用户使用 `/compact` 或切换模型

**验证命令**：
```bash
# 查看错误处理
tail -f ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep -E "(Layer-3.*failed|BAD_REQUEST)"
```

---

## 性能验证

### Token 成本节省

**测试方法**：
1. 记录压缩前的 Token 用量（从日志中提取）
2. 记录压缩后的 Token 用量
3. 计算节省比例

**预期结果**：
- Layer 1: 60-90% 节省
- Layer 2: 70-95% 节省
- Layer 3: 86-97% 节省

### 响应速度

**测试方法**：
1. 使用 `time` 命令测量响应时间
2. 对比压缩前后的响应速度

**预期结果**：
- Layer 1/2: 响应速度无明显变化
- Layer 3: 首次摘要生成可能增加 2-5 秒，后续请求正常

---

## 兼容性测试

### 客户端兼容性

测试以下客户端：
- ✅ Claude Code CLI
- ✅ Cherry Studio
- ✅ Cursor
- ✅ Python OpenAI SDK
- ✅ Kilo Code

### 模型兼容性

测试以下模型：
- ✅ Gemini 3 Flash
- ✅ Gemini 3 Pro High
- ✅ Claude 4.5 Sonnet
- ✅ Claude 4.5 Opus Thinking

---

## 回归测试

### 签名链完整性

**验证点**：
- Layer 2 压缩后签名不丢失
- Layer 3 Fork 后签名正确恢复
- 无 400 签名错误

### 工具调用链

**验证点**：
- 工具调用在压缩后仍能正常工作
- 工具结果正确传递
- 无工具调用中断

---

## 日志分析

### 关键日志模式

```bash
# Layer 1 触发
grep "Layer-1.*Tool trimming" antigravity.log

# Layer 2 触发
grep "Layer-2.*Thinking compression" antigravity.log

# Layer 3 触发
grep "Layer-3.*Fork successful" antigravity.log

# Token 节省统计
grep "Compression result.*saved" antigravity.log
```

---

## 测试报告模板

```markdown
## 测试结果

### 场景 1: Layer 1 工具消息裁剪
- [ ] 触发成功
- [ ] 保留最近 5 轮
- [ ] 无 400 错误
- [ ] 响应速度正常

### 场景 2: Layer 2 Thinking 压缩
- [ ] 触发成功
- [ ] 签名完整保留
- [ ] 无签名错误
- [ ] 压缩率达标

### 场景 3: Layer 3 Fork 会话
- [ ] 触发成功
- [ ] XML 摘要生成
- [ ] 压缩率 86-97%
- [ ] 无 Cache 破坏

### 场景 4: 渐进式触发
- [ ] 顺序正确 (1→2→3)
- [ ] Token 重新估算
- [ ] 日志清晰

### 场景 5: 错误处理
- [ ] 失败时友好提示
- [ ] 无崩溃
- [ ] 建议明确

### 性能验证
- Token 节省: ____%
- 响应速度: 正常/慢 (___ms)

### 兼容性
- Claude Code: ✅/❌
- Cherry Studio: ✅/❌
- Cursor: ✅/❌
- Python SDK: ✅/❌

### 回归测试
- 签名链完整: ✅/❌
- 工具调用正常: ✅/❌

## 问题记录

(记录测试中发现的问题)

## 结论

(总体评价和建议)
```

---

## 快速测试脚本

```bash
#!/bin/bash
# 快速测试三层压缩

echo "=== 测试 Layer 1 (工具裁剪) ==="
# 使用 Claude Code 执行多次文件搜索
claude "请搜索项目中所有 .rs 文件，然后读取其中 5 个文件的内容"

echo "=== 测试 Layer 2 (Thinking 压缩) ==="
# 使用 Thinking 模型进行复杂推理
claude --model claude-opus-4-5-thinking "请详细分析这段代码的性能瓶颈并提出优化方案"

echo "=== 测试 Layer 3 (Fork 会话) ==="
# 超长对话触发 Fork
for i in {1..20}; do
  claude "继续上一个话题，请提供更多细节 (第 $i 轮)"
done

echo "=== 查看日志 ==="
tail -100 ~/Library/Application\ Support/com.antigravity.tools/logs/antigravity.log | grep -E "Layer-[123]"
```

---

## 注意事项

1. **测试环境**：确保在干净的环境中测试，避免其他因素干扰
2. **日志级别**：必须设置 `RUST_LOG=debug` 才能看到详细日志
3. **账号配额**：测试前确保账号有足够配额
4. **备份数据**：测试前备份重要数据
5. **版本确认**：确认运行的是 v3.3.50 版本

---

## 问题排查

### 问题 1：Layer 1 未触发
- 检查对话是否达到 60% 压力
- 查看 Token 估算是否准确

### 问题 2：Layer 2 签名丢失
- 检查 `compress_thinking_preserve_signature` 函数
- 验证签名提取逻辑

### 问题 3：Layer 3 摘要失败
- 检查 Gemini 账号是否可用
- 验证 `call_gemini_sync` 函数
- 查看上游 API 错误

### 问题 4：400 错误
- 检查签名链是否完整
- 验证工具调用参数
- 查看上游 API 响应

---

## 联系方式

如有问题，请在 GitHub 提 Issue：
https://github.com/lbjlaq/Antigravity-Manager/issues
