# Antigravity Tools 发版操作指南 (Release SOP)

发版详细规程。提交门禁与发版红线以根目录 `AGENTS.md` 为准。

---

## 一、流程概览

```text
[1] 预检 · 打版                      [2] 填日志              [3] 提交 · 打 Tag · 推送
Pre-flight Checks ─► npm run bump ─► 编辑 CHANGELOG.md ─► git push origin main
                                                          git tag vX.Y.Z && git push origin vX.Y.Z
                                                                     │
                                                                     ▼
                                       GitHub Actions 构建全平台安装包 + Docker 镜像，生成 Releases
```

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
cargo check
cd ..
npm run build
```

> `ci.yml` 仅门禁 `main` 推送与面向 `main` 的 PR。tag 若打在 feature 分支上，Release 工作流不受 CI 门禁保护，须由本地预检兜底。

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

**同步范围**：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`、`Casks/antigravity-tools.rb`、`README.md`、`README_EN.md`、`src/components/layout/MiniView.tsx`、`src/pages/Settings.tsx`、`CHANGELOG.md`、`CHANGELOG_EN.md`。

**内置防呆**：目标版本必须严格高于当前版本，否则红色拦截，杜绝版本回退。

**预发布版本号形态**：`npm run bump beta` 生成 `X.Y.Z-beta.N`（首次为 `beta.1`，后续递增为 `beta.2`）。该版本串同时决定脚本插入的 CHANGELOG 骨架标题与后续 Tag 名，**三者必须完全一致**（详见第 2 步提示与第 4 步）。

### 第 2 步：补充更新日志

在脚本插入的版本骨架中填写核心亮点：

```markdown
*   **版本演进**:
    *   **v4.7.14 (2026-09-22)**:
        -   **[核心分类] 更新标题 (PR #xxx)**:
            -   **功能详述**: 该版本修复的核心问题或新增能力。
        -   **[核心分类] 涉及外部贡献的条目 (Fixes #xxxx, Thanks to @username)**:
            -   **功能详述**: 致谢以行内形式写在对应条目上。
```

> 1. **标题必须与 Tag 逐字符一致**：流水线用 `awk` 以 tag 名（`github.ref_name`，含 `v` 前缀）匹配 CHANGELOG 标题行，**`v` 前缀与完整预发布后缀都要一字不差**。`npm run bump beta` 自增出的版本号形如 `X.Y.Z-beta.1`，因此 Tag 应为 `vX.Y.Z-beta.1`（而非 `vX.Y.Z-beta`），标题也须写成 `**vX.Y.Z-beta.1 (日期)**`。不匹配时正文会静默退化为占位文案 `See the assets to download this version and install.`。
> 2. 已开启 `generateReleaseNotes: true`，GitHub 会自动追加 `What's Changed` 与 `New Contributors`（含 PR 链接与贡献者主页）。
> 3. **测试版不进入 README**：Tag 含 `-` 的预发布 / 衍生版本（`-beta` / `-cleaned` / `-rc` 等）**只在 `CHANGELOG.md` 记录**，不得写入任何 README 的版本号、Shields 徽章或「最新版本」段落。README 始终只反映最新**正式版**。`bump-version.mjs` 已内置该判定：预发布版本自动跳过两个 README，仅同步其余版本配置文件。
> 4. **贡献者致谢写在条目行内**：不单列致谢块，外部贡献者统一以 `(Thanks to @username)` 标注在对应条目上。Release 页的 **Contributors 头像列表由正文中的 `@username` 自动生成** —— 增删提及即增删头像，条目内没有 `@username` 时该列表为空。

### 第 3 步：提交并推送主干

```bash
git add -A
git commit -m "chore(release): bump version to 4.7.14 and update changelog"
git push origin main
```

### 第 4 步：打 Tag 并推送

```bash
git tag v4.7.14              # 正式版：注意带 'v' 前缀
git push origin v4.7.14      # 触发 Release 工作流

git tag v4.7.14-beta.1       # 预发布：须与 CHANGELOG 标题逐字符一致（含 .N 序号）
git push origin v4.7.14-beta.1
```

> Tag 串必须与 CHANGELOG 中该版本的标题**逐字符相同**（`v` 前缀 + 完整预发布后缀），否则 Release 正文会退化为占位文案。

**预发布标签规则**：Tag 名含 `-`（如 `v4.7.14-beta.1`、`v4.7.13-cleaned`、`v4.8.0-rc.1`）时，Release 自动标记为 **Pre-release** 且**不更新 Latest**，不会经 `releases/latest/download/updater.json` 推送给正式用户，可放心用于灰度与内测；纯 `vX.Y.Z` 按正式版发布并更新 Latest。

**从 feature 分支发 Beta**：tag 不绑定分支，可直接对分支上的提交打 tag。此时须先本地跑完第 0 步预检（该提交不经过 CI 门禁）。

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
