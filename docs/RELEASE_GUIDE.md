# Antigravity Tools 发版操作指南 (Release SOP)

发版详细规程。提交门禁与发版红线以根目录 `AGENTS.md` 为准。

---

## 一、流程概览

```text
[通道 A: 正式版发布]
Pre-flight ─► checkout main ─► npm run bump <patch|minor> ─► 补充日志 ─► git push origin main ─► 打 Tag vX.Y.Z ─► 自动发布 Latest Release

[通道 B: Beta 预发布]
Pre-flight ─► checkout beta ─► npm run bump beta ──────────► 补充日志 ─► git push origin beta ─► 打 Tag vX.Y.Z-beta.N ─► 自动发布 Pre-release (隔离无感)
```

> **通道隔离与维护者协作原则**：
> - **正式版通道 (Main)**：`main` 为**绝对纯净正式打版分支**，仅发布纯数字正式版本（如 `v4.7.14`），流水线严格拦截任何带 `-` 的预发标签。
> - **预览版通道 (Beta)**：`beta` 为**独立预发布打版分支**，所有预发测试版本（如 `v4.7.14-beta.1`、`-cleaned` 等）在此提交并由 `beta` 触发独立构建。预发布产物自动标记为 Pre-release 且绝不打 Latest，完全不影响正式版主用户更新。
> - **新更改优先暂存验证 (Staging on Beta First)**：凡涉及新功能、重大重构或高风险修复，**必须主动询问维护者**是否先在 `beta` 分支进行修改与验证。待验证稳定（或发布 Beta 预览版内测确认）后，方可合并进入 `main`。

---

## 二、操作步骤

### 第 0 步：发版前预检 (Pre-flight)

确保工作区干净，且**对将被标记的提交**执行与 CI 完全一致的预检命令：

```bash
git checkout main && git pull origin main
git status          # 应显示 nothing to commit, working tree clean

cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets --all-features
cd ..
npm run build
```

> CI 门禁已全量覆盖 `main` 与 `beta` 分支。正式发布前确保在 `main` 预检通过，Beta 预发前确保在 `beta` 预检通过。

### 第 1 步：版本号原子同步

`scripts/bump-version.mjs` 一键同步全仓库版本号并生成 CHANGELOG 骨架：

| 场景 | 命令 | 示例 |
| --- | --- | --- |
| 补丁（Bugfix / 性能） | `npm run bump patch` | 4.7.13 → 4.7.14 |
| 次版本（新增特性） | `npm run bump minor` | 4.7.13 → 4.8.0 |
| 主版本（破坏性变更） | `npm run bump major` | 4.7.13 → 5.0.0 |
| 预发布递增 | `npm run bump beta` | 4.7.13 → 4.7.14-beta.1（beta.1 → beta.2） |
| 指定衍生版本 | `npm run bump 4.7.14-cleaned` | 同基线双版本（`-beta` / `-cleaned` / `-rc`） |
| 指定任意合法 SemVer | `npm run bump 4.8.0` | — |

**可选参数**：`--dry-run` 仅演练、不写盘；`--commit` 自动生成 `chore(release): bump version to ...` 提交。

**同步范围**：`package.json`、`package-lock.json`（根版本镜像，两处）、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`、`Casks/antigravity-tools.rb`、`README.md`、`README_EN.md`、`src/components/layout/MiniView.tsx`、`src/pages/Settings.tsx`、`CHANGELOG.md`、`CHANGELOG_EN.md`。

> 版本串匹配采用**结构锚定**而非精确匹配当前版本号：`package-lock.json` 按字段位置锚定，README 按 `(v数字…)` / `Version-数字…-blue` 形态锚定。这样即使历史轮次（如预发布跳过 README）造成版本串滞后，下一轮同步仍能正确命中。

**内置防呆**：目标版本必须严格高于当前版本，否则红色拦截，杜绝版本回退。

**预发布版本号形态**：`npm run bump beta` 生成 `X.Y.Z-beta.N`（首次为 `beta.1`，后续递增为 `beta.2`）。该版本串同时决定脚本插入的 CHANGELOG 骨架标题与后续 Tag 名，**三者必须完全一致**（详见第 2 步提示与第 4 步）。

### 第 2 步：回溯提交与补充更新日志

在填写更新日志前，**必须以 Git 提交历史与已合入 PR 为客观事实依据进行完整回溯**，严防遗漏贡献者署名或 Issue 关联：

```bash
# 1. 扫描上个版本以来的全部提交、Author 与 Co-Authored-By 署名
git log $(git describe --tags --abbrev=0)..HEAD --format="Commit: %h | %an <%ae> | %s%n%(trailers:key=Co-Authored-By)"

# 2. 列出在此期间合并的 PR 与关联 Issue
gh pr list --state merged --limit 20
```

根据盘点结果，在脚本插入的版本骨架中填写核心亮点：

- **强制关联 Issue / PR**：条目标题必须包含对应的来源单号（如 `(PR #3504)` 或 `(Fixes #3499, #3501)`）；
- **强制行内致谢贡献者**：从提交历史和 PR 中识别出的所有外部贡献者，必须以 `(Thanks to @username)` 形式显式标注在对应条目上。Release 页面的 **Contributors 头像列表由此自动提取生成**；
- **格式示例**：
```markdown
*   **版本演进**:
    *   **v4.7.14 (2026-09-23)**:
        -   **[核心分类] 功能重构与优化 (PR #3504)**:
            -   **功能详述**: 核心实现说明。
        -   **[核心分类] 涉及外部贡献的修复 (Fixes #3508, Thanks to @username)**:
            -   **功能详述**: 致谢与修复说明。
```

> 1. **标题必须与 Tag 逐字符一致**：流水线用 `awk` 以 tag 名（`github.ref_name`，含 `v` 前缀）匹配 CHANGELOG 标题行，**`v` 前缀与完整预发布后缀都要一字不差**。`npm run bump beta` 自增出的版本号形如 `X.Y.Z-beta.1`，因此 Tag 应为 `vX.Y.Z-beta.1`（而非 `vX.Y.Z-beta`），标题也须写成 `**vX.Y.Z-beta.1 (日期)**`。不匹配时正文会静默退化为占位文案 `See the assets to download this version and install.`。
> 2. 已开启 `generateReleaseNotes: true`，GitHub 会自动追加 `What's Changed` 与 `New Contributors`（含 PR 链接与贡献者主页）。
> 3. **测试版不进入 README**：Tag 含 `-` 的预发布 / 衍生版本（`-beta` / `-cleaned` / `-rc` 等）**只在 `CHANGELOG.md` 记录**，不得写入任何 README 的版本号、Shields 徽章或「最新版本」段落。README 始终只反映最新**正式版**。`bump-version.mjs` 已内置该判定：预发布版本自动跳过两个 README，仅同步其余版本配置文件。
> 4. **贡献者致谢写在条目行内**：不单列致谢块，外部贡献者统一以 `(Thanks to @username)` 标注在对应条目上。Release 页的 **Contributors 头像列表由正文中的 `@username` 自动生成** —— 增删提及即增删头像，条目内没有 `@username` 时该列表为空。
> 5. **正式版发版强制同步双语 README 更新日志**：发布正式版时，除了更新 `CHANGELOG.md`（及 `CHANGELOG_EN.md`），还必须同步将最新正式版的重要更新摘要更新至 `README.md`（`## 📝 更新日志`）与 `README_EN.md`（`## 📝 Changelog`），严禁仅更新 CHANGELOG 而遗漏 README 首页的最新版本说明。

### 第 3 步：提交并推送目标分支

```bash
# 正式版：提交并推送到 main 分支
git checkout main
git add -A
git commit -m "chore(release): bump version to 4.7.14 and update changelog"
git push origin main

# Beta 预发版：提交并推送到 beta 分支（严禁推到 main，保持 main 纯净）
git checkout beta
git add -A
git commit -m "chore(release): bump version to 4.7.14-beta.1 and update changelog"
git push origin beta
```

### 第 4 步：打 Tag 并推送

```bash
# 正式版：从 main 打纯数字 Tag（触发正式发布，更新 Latest）
git tag v4.7.14
git push origin v4.7.14

# Beta 预发版：从 beta 打预发布 Tag（触发隔离构建，不更新 Latest）
git tag v4.7.14-beta.1
git push origin v4.7.14-beta.1
```

> Tag 串必须与 CHANGELOG 中该版本的标题**逐字符相同**（`v` 前缀 + 完整预发布后缀），否则 Release 正文会退化为占位文案。

**分支与标签严格门禁**：流水线内置分支与标签匹配断言。
- 带有 `-` 的预发布 Tag 若打在 `main` 独有提交上，流水线立即拦截阻断，拒绝构建与发布；
- 不带 `-` 的正式版 Tag 若打在 `beta` 独有提交上，流水线立即拦截阻断。
- 预发布版本自动降级为 NSIS、不更新 Latest、不更新正式用户的 `updater.json`，主用户客户端绝不受任何影响。

### 第 5 步：验收

推送 Tag 后流水线自动接管，无需人工干预：

1. **进度**：仓库 `Actions` 页的 `Release` 工作流；
2. **构建矩阵**：Windows（`.msi` / NSIS `.exe`）、macOS（`.dmg`，Apple Silicon 与 Intel 双架构）、Linux（`.AppImage` / `.deb` / `.rpm`）、Docker 多架构镜像推送 Docker Hub；产出 `updater.json`（配置 `TAURI_SIGNING_PRIVATE_KEY` 时含签名）；
3. **验收**：约 10~15 分钟后在 `Releases` 页确认 `Antigravity Tools vX.Y.Z` 及附件齐全。

---

## 三、异常与救急

### 1. 删除误打的 Tag

```bash
git tag -d v4.7.14
git push origin :refs/tags/v4.7.14
```

### 2. 防呆保护报错

报错 `✗ 错误: 防呆保护生效：目标版本号 [...] 必须严格高于当前版本号 [...]！` 说明目标版本小于或等于当前版本。版本号必须严格单调递增，请传入更高版本（如 `npm run bump patch`）。
