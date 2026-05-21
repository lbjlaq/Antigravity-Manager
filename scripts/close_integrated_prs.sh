#!/bin/bash

# 关闭已集成到 v4.0.3 的 PR 脚本
# 使用前请确保已安装并登录 GitHub CLI: brew install gh && gh auth login

REPO="lbjlaq/Antigravity-Manager"
VERSION="v4.0.3"

# 感谢消息模板
THANK_YOU_MESSAGE="感谢您的贡献！🎉

此 PR 的更改已被手动集成到 ${VERSION} 版本中。

相关更新已包含在以下文件中：
- README.md 的版本更新日志
- 贡献者列表

再次感谢您对 Antigravity Tools 项目的支持！

---

Thank you for your contribution! 🎉

The changes from this PR have been manually integrated into ${VERSION}.

The updates are documented in:
- README.md changelog
- Contributors list

Thank you again for your support of the Antigravity Tools project!"

echo "================================================"
echo "关闭已集成到 ${VERSION} 的 PR"
echo "================================================"
echo ""

# PR 列表：格式为 "PR号|作者|标题"
PRS_LIST=(
    "825|IamAshrafee|[Internationalization] Device Fingerprint Dialog localization"
    "822|Koshikai|[Japanese] Add missing translations and refine terminology",
    "798|vietnhatthai|[Translation Fix] Correct spelling error in Vietnamese settings",
    "846|lengjingxu|[核心功能] 客户端热更新与 Token 统计系统",
    "949|lbjlaq|Streaming chunks order fix",
    "950|lbjlaq|[Fix] Remove redundant code and update README",
    "973|Mag1cFall|fix: 修复 Windows 平台启动参数不生效的问题"
)

# 检查 GitHub CLI 是否已安装
if ! command -v gh &> /dev/null; then
    echo "❌ GitHub CLI 未安装"
    echo ""
    echo "请先安装 GitHub CLI:"
    echo "  brew install gh"
    echo ""
    echo "然后登录:"
    echo "  gh auth login"
    echo ""
    exit 1
fi

# 检查是否已登录
if ! gh auth status &> /dev/null; then
    echo "❌ 未登录 GitHub CLI"
    echo ""
    echo "请先登录:"
    echo "  gh auth login"
    echo ""
    exit 1
fi

echo "✅ GitHub CLI 已就绪"
echo ""

# 遍历并处理每个 PR
for item in "${PRS_LIST[@]}"; do
    PR_NUM=$(echo "$item" | cut -d'|' -f1)
    AUTHOR=$(echo "$item" | cut -d'|' -f2)
    TITLE=$(echo "$item" | cut -d'|' -f3)
    
    echo "----------------------------------------"
    echo "处理 PR #${PR_NUM}: ${TITLE}"
    echo "作者: @${AUTHOR}"
    echo "----------------------------------------"
    
    # 添加感谢评论
    echo "📝 添加感谢评论..."
    gh pr comment ${PR_NUM} --repo ${REPO} --body "${THANK_YOU_MESSAGE}"
    
    if [ $? -eq 0 ]; then
        echo "✅ 评论已添加"
    else
        echo "❌ 评论添加失败"
        continue
    fi
    
    # 关闭 PR
    echo "🔒 关闭 PR..."
    gh pr close ${PR_NUM} --repo ${REPO} --comment "已集成到 ${VERSION}，关闭此 PR。"
    
    if [ $? -eq 0 ]; then
        echo "✅ PR #${PR_NUM} 已关闭"
    else
        echo "❌ PR #${PR_NUM} 关闭失败"
    fi
    
    echo ""
    sleep 2  # 避免 API 限流
done

echo "================================================"
echo "✅ 所有 PR 处理完成！"
echo "================================================"
echo ""
echo "请访问以下链接查看结果："
echo "https://github.com/${REPO}/pulls?q=is%3Apr+is%3Aclosed"
