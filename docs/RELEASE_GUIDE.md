# 🚀 Antigravity Tools 标准发版操作指南 (Release SOP)

本文档记录了 Antigravity Tools 项目的工业级标准发版操作规程。借助内置的自动化工具，整个发版过程已被收敛为极简的自动化三步流。

---

## 一、 整体发版时序全景图

```text
  【第 1 步: 一键打版】            【第 2 步: 填写日志】             【第 3 步: 打 Tag 自动发布】
   npm run bump patch   ──►  编辑 CHANGELOG.md  ──►  git push origin main
 (自动原子化更新11处文件)       (填充自动生成的骨架)           git tag vX.Y.Z && git push origin vX.Y.Z
                                                                     │
                                                                     ▼
                                                   GitHub Actions 自动构建全平台安装包、
                                                   构建 Docker 镜像、自动生成 Releases 页面！
```

---

## 二、 详细操作步骤

### 第 0 步：发版前自检 (Pre-flight Checklist)
确保本地 `main` 分支是最新的，且工作区干净：
```bash
git checkout main
git pull origin main
git status # 应显示: nothing to commit, working tree clean
```

---

### 第 1 步：一键版本升级 (The One-Click Bump)

使用项目内置的原子化版本同步脚本，自动计算新版本并一键修改全仓库 11 处配置文件与文档：

```bash
# 场景 A: 补丁版本升级 (例如 4.7.13 -> 4.7.14) [日常 Bugfix / 性能调优]
npm run bump patch

# 场景 B: 次版本号升级 (例如 4.7.13 -> 4.8.0) [引入重大新特性]
npm run bump minor

# 场景 C: 预发布版本递增 (例如 4.7.13 -> 4.7.14-beta.1，或 beta.1 -> beta.2)
npm run bump beta

# 场景 D: 发布特定测试/衍生双版本 (例如针对纯透传分支发布 4.7.13-cleaned 或 4.7.13-beta)
npm run bump 4.7.13-cleaned
npm run bump 4.7.13-beta

# 场景 E: 指定任意合法 SemVer 版本号
npm run bump 4.8.0

# [可选安全演练]: 仅测试检查，不写入磁盘
npm run bump patch --dry-run
```

#### 脚本自动执行的动作清单：
- [x] **防呆递增校验**：校验 `目标版本 > 当前版本`，若手误输入等于或小于当前版本号，直接红色拦截报错，阻止版本回退；
- [x] `package.json`：更新 `"version"`；
- [x] `src-tauri/Cargo.toml`：更新 `version`；
- [x] `src-tauri/tauri.conf.json`：更新 `"version"`；
- [x] `src-tauri/Cargo.lock`：原子化同步包版本并联动 `cargo check` 校验依赖图；
- [x] `Casks/antigravity-tools.rb`：更新 Homebrew Cask 版本定义；
- [x] `README.md` & `README_EN.md`：自动同步标题版本号与 Shields 徽章；
- [x] `src/components/layout/MiniView.tsx` & `src/pages/Settings.tsx`：更新前端版本展示兜底；
- [x] `CHANGELOG.md` & `CHANGELOG_EN.md`：自动在顶部插入带当前日期的版本骨架占位。

---

### 第 2 步：补充更新日志 (Edit Changelog)

打开 `CHANGELOG.md`（以及可选的 `CHANGELOG_EN.md`），在脚本自动插入的最顶部骨架中，填入本次版本的核心更新亮点：

```markdown
*   **版本演进**:
    *   **v4.7.14 (2026-09-22)**:
        -   **[核心分类] 核心更新标题 (PR #xxx)**:
            -   **功能详述**: 描述该版本修复的核心问题或新增功能。
        -   **🤝 v4.7.14 核心贡献者致谢 (Contributors)**:
            -   特别感谢以下贡献者对 v4.7.14 版本的研发与技术贡献:
                *   @jeikl (主导本次版本核心架构)
                *   @JeikCode (全流程 AI 协同架构与代码实现, Co-authored)
                *   @contributor (PR #xxx: 贡献说明)
```

> **提示**：
> 1. GitHub Actions 流水线会自动使用 `awk` 提取该版本号下方的内容作为前言说明；
> 2. 云端发布流水线已开启 `generateReleaseNotes: true`，GitHub 官方会自动在说明正文下方追加 **`What's Changed` 与 `New Contributors` 完整列表**（包含所有 PR 链接与贡献者头像/主页致谢）。

---

### 第 3 步：提交发版准备并推送到主干 (Commit & Push Main)

```bash
# 提交所有版本号与日志修改
git add -A
git commit -m "chore(release): bump version to 4.7.10 and update changelog

Co-Authored-By: JeikCode <331041501+JeikCode@users.noreply.github.com>"

# 推送到官方主分支
git push origin main
```

---

### 第 4 步：打上官方 Tag 并推送（触发全自动发布流水线）

推送到主干后，打上对应的 `v*` 格式标签并推送：

```bash
# 打上 Git Tag (注意带上 'v' 前缀)
git tag v4.7.10

# 推送 Tag 到官方远程仓库 (此时 GitHub Actions 自动化流水线正式启动！)
git push origin v4.7.10
```

---

### 第 5 步：坐等出厂与验收 (Release Verification)

推送 Tag 后，无需任何人工干预，云端流水线会自动接管：
1. **进度查看**：访问仓库的 `Actions` 标签页，会看到名为 `Release` 的工作流正在运行；
2. **自动化构建矩阵**：
   - 自动制作 Windows x64 安装包（`.exe` 与 NSIS 安装向导）；
   - 自动制作 macOS 苹果应用（`.dmg` 磁盘映像，支持 Apple Silicon M系列与 Intel 双架构）；
   - 自动制作 Linux 安装包（`.AppImage` 与 `.rpm`）；
   - 自动构建 Docker 多架构镜像并推送到 Docker Hub；
   - 自动对所有二进制文件执行密码学签名并生成 `updater.json`；
3. **最终产物验证**：
   约 10~15 分钟后，访问仓库的 **`Releases`** 页面，最新版本的 `Antigravity Tools vX.Y.Z` 已经上架，全套可下载文件与更新日志已完整呈现！

---

## 三、 异常情况与救急指南

### 1. 手误打错了 Tag 怎么办？（如何撤销未发布的 Tag）
如果手误打错了 Tag 且已经推送到远程：
```bash
# 本地删除 Tag
git tag -d v4.7.10

# 远程删除 Tag
git push origin :refs/tags/v4.7.10
```

### 2. 为什么提示防呆保护报错？
如果运行 `npm run bump` 时报错：
> `✗ 错误: 防呆保护生效：目标版本号 [4.7.9] 必须严格高于当前版本号 [4.7.9]！`

这说明目标版本号小于或等于现有版本。发版版本号必须严格单调递增，请传入更高的版本号（如 `npm run bump patch`）。
