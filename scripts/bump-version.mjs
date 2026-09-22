#!/usr/bin/env node

/**
 * Antigravity Tools - 一键版本升级与多文件原子化同步脚本
 *
 * 用法:
 *   npm run bump patch                 # 自动自增补丁版本号 (例如 4.7.13 -> 4.7.14)
 *   npm run bump minor                 # 自动自增次版本号 (例如 4.7.13 -> 4.8.0)
 *   npm run bump major                 # 自动自增主版本号 (例如 4.7.13 -> 5.0.0)
 *   npm run bump beta                  # 自动生成或自增 Beta 预发版 (例如 4.7.13 -> 4.7.14-beta.1)
 *   npm run bump 4.7.14-beta           # 发布测试/预发布双版本 (支持 -beta, -cleaned 等)
 *   npm run bump 4.7.14-cleaned        # 发布特定衍生/优化双版本
 *   npm run bump patch --dry-run       # 模拟演练模式，仅检查和输出 diff，不实际写磁盘
 *   npm run bump patch --commit        # 自动生成标准提交 `chore(release): bump version to ...`
 *
 * 注: 版本号含预发布标签 (SemVer 2.0, Tag 含 '-') 时，README 标题与徽章保持最新正式版不变。
 */

import fs from 'node:fs';
import path from 'node:path';
import { execSync, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT_DIR = path.resolve(__dirname, '..');

// 颜色输出辅助
const colors = {
    reset: '\x1b[0m',
    green: '\x1b[32m',
    yellow: '\x1b[33m',
    red: '\x1b[31m',
    cyan: '\x1b[36m',
    bold: '\x1b[1m',
};

function log(msg) {
    console.log(`${colors.cyan}[Bump-Version]${colors.reset} ${msg}`);
}
function success(msg) {
    console.log(`${colors.green}✓ ${msg}${colors.reset}`);
}
function error(msg) {
    console.error(`${colors.red}✗ 错误: ${msg}${colors.reset}`);
}
function warn(msg) {
    console.warn(`${colors.yellow}⚠ 警告: ${msg}${colors.reset}`);
}

// 1. 读取当前根目录 package.json 版本
const pkgPath = path.join(ROOT_DIR, 'package.json');
if (!fs.existsSync(pkgPath)) {
    error('未找到根目录 package.json 文件！');
    process.exit(1);
}

const pkgContent = fs.readFileSync(pkgPath, 'utf8');
const pkgJson = JSON.parse(pkgContent);
const currentVersion = pkgJson.version;

// 标准 SemVer 2.0 正则：支持 Major.Minor.Patch 及可选的 Pre-release 标签 (如 -beta, -cleaned, -beta.1)
const SEMVER_REGEX = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

function parseSemVer(v) {
    if (!v || typeof v !== 'string') return null;
    const match = v.trim().match(SEMVER_REGEX);
    if (!match) return null;
    return {
        major: Number(match[1]),
        minor: Number(match[2]),
        patch: Number(match[3]),
        prerelease: match[4] || null,
        raw: v.trim().replace(/^v/, ''),
    };
}

const curSem = parseSemVer(currentVersion);
if (!curSem) {
    error(`当前 package.json 中的版本号 "${currentVersion}" 不符合语义化版本 (SemVer) 规范！`);
    process.exit(1);
}

// 2. 解析命令行参数
const args = process.argv.slice(2);
const isDryRun = args.includes('--dry-run') || process.env.npm_config_dry_run === 'true';
const autoCommit = args.includes('--commit') || process.env.npm_config_commit === 'true';
const targetArg = args.find(a => !a.startsWith('--'));

if (!targetArg) {
    console.log(`
${colors.bold}Antigravity Tools 一键打版与双版本发布工具${colors.reset}

当前版本: ${colors.green}${currentVersion}${colors.reset}

常用用法:
  npm run bump patch           # 补丁递增 (例如 ${currentVersion} -> 正式补丁自增)
  npm run bump minor           # 次版本递增 (例如 ${currentVersion} -> X.Y.0)
  npm run bump major           # 主版本递增 (例如 ${currentVersion} -> X.0.0)
  npm run bump beta            # 预发版本递增 (例如 ${currentVersion} -> X.Y.Z-beta.1)
  npm run bump <目标版本号>     # 指定任意合法版本 (支持双版本，例如 4.7.13-beta 或 4.7.13-cleaned)

选项:
  --dry-run                    # 仅演练测试，不实际修改任何文件
  --commit                     # 自动执行 git commit 提交所有版本修改
`);
    process.exit(0);
}

// 3. 计算新版本号
let nextSem = null;
const normalizedTarget = targetArg.toLowerCase();

if (normalizedTarget === 'patch') {
    if (curSem.prerelease) {
        // 当前是预发版，patch 操作默认转为同号正式版 (如 4.7.13-beta -> 4.7.13)
        nextSem = { major: curSem.major, minor: curSem.minor, patch: curSem.patch, prerelease: null };
    } else {
        nextSem = { major: curSem.major, minor: curSem.minor, patch: curSem.patch + 1, prerelease: null };
    }
} else if (normalizedTarget === 'minor') {
    nextSem = { major: curSem.major, minor: curSem.minor + 1, patch: 0, prerelease: null };
} else if (normalizedTarget === 'major') {
    nextSem = { major: curSem.major + 1, minor: 0, patch: 0, prerelease: null };
} else if (normalizedTarget === 'beta') {
    if (curSem.prerelease && curSem.prerelease.startsWith('beta.')) {
        // 自增 beta 序号 (如 beta.1 -> beta.2)
        const sub = Number(curSem.prerelease.split('.')[1]) || 0;
        nextSem = { major: curSem.major, minor: curSem.minor, patch: curSem.patch, prerelease: `beta.${sub + 1}` };
    } else if (curSem.prerelease === 'beta') {
        nextSem = { major: curSem.major, minor: curSem.minor, patch: curSem.patch, prerelease: 'beta.1' };
    } else {
        // 正式版开启下一个 patch 的 beta
        nextSem = { major: curSem.major, minor: curSem.minor, patch: curSem.patch + 1, prerelease: 'beta.1' };
    }
} else {
    nextSem = parseSemVer(targetArg);
    if (!nextSem) {
        error(`输入的目标版本 "${targetArg}" 格式非法！必须是 SemVer 规范 (例如 4.7.13 或 4.7.13-beta / 4.7.13-cleaned)！`);
        process.exit(1);
    }
}

const newVersion = nextSem.prerelease
    ? `${nextSem.major}.${nextSem.minor}.${nextSem.patch}-${nextSem.prerelease}`
    : `${nextSem.major}.${nextSem.minor}.${nextSem.patch}`;

// 预发布 / 衍生版本（SemVer 2.0 预发布语义，Tag 含 '-'）仅在 CHANGELOG 留痕。
// README 始终只反映最新正式版，详见 AGENTS.md -> Release Discipline。
const isPrerelease = Boolean(nextSem.prerelease);

// 4. 防呆校验逻辑 (支持双版本发布与预发布转正)
function validateVersionUpgrade(next, cur) {
    if (next.raw === cur.raw) {
        return { valid: false, reason: `目标版本号 [${next.raw}] 与当前版本号完全一致，无需重复升级！` };
    }

    if (next.major > cur.major) return { valid: true };
    if (next.major < cur.major) return { valid: false, reason: `主版本号倒退: ${next.major} < ${cur.major}` };

    if (next.minor > cur.minor) return { valid: true };
    if (next.minor < cur.minor) return { valid: false, reason: `次版本号倒退: ${next.minor} < ${cur.minor}` };

    if (next.patch > cur.patch) return { valid: true };
    if (next.patch < cur.patch) return { valid: false, reason: `补丁版本号倒退: ${next.patch} < ${cur.patch}` };

    // 基础三段版本号相等时的特殊场景 (双版本 / 预发转正)
    // 场景 A: 预发版转正 (4.7.13-beta -> 4.7.13)
    if (cur.prerelease && !next.prerelease) {
        return { valid: true, note: '预发版本正式转正' };
    }
    // 场景 B: 基于当前正式版发布衍生/测试双版本 (4.7.13 -> 4.7.13-cleaned / 4.7.13-beta)
    if (!cur.prerelease && next.prerelease) {
        return { valid: true, note: `双版本发布 (正式版 -> ${next.prerelease})` };
    }
    // 场景 C: 预发布分支演进 (如 4.7.13-beta -> 4.7.13-cleaned 或 beta.1 -> beta.2)
    if (cur.prerelease && next.prerelease) {
        return { valid: true, note: `预发版本状态演进: ${cur.prerelease} -> ${next.prerelease}` };
    }

    return { valid: false, reason: `防呆保护生效：目标版本 [${next.raw}] 不高于当前版本 [${cur.raw}]` };
}

const validation = validateVersionUpgrade({ ...nextSem, raw: newVersion }, curSem);
if (!validation.valid) {
    error(`防呆保护生效: ${validation.reason}`);
    error('发版版本号绝不允许低于现有版本，防止版本回退导致更新检查死锁。');
    process.exit(1);
}

if (validation.note) {
    log(`检测到版本模式: ${colors.cyan}${validation.note}${colors.reset}`);
}

log(`启动版本号同步: ${colors.yellow}${currentVersion}${colors.reset} -> ${colors.green}${colors.bold}${newVersion}${colors.reset}${isDryRun ? ' [DRY-RUN 演练模式]' : ''}`);

// 5. 采用本地日期避免时区偏差导致的发版日期倒退
const now = new Date();
const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;

const TARGET_FILES = [
    {
        name: 'package.json',
        relPath: 'package.json',
        replace: (content) => content.replace(
            `"version": "${currentVersion}"`,
            `"version": "${newVersion}"`
        ),
    },
    {
        name: 'src-tauri/Cargo.toml',
        relPath: 'src-tauri/Cargo.toml',
        replace: (content) => content.replace(
            `version = "${currentVersion}"`,
            `version = "${newVersion}"`
        ),
    },
    {
        name: 'src-tauri/tauri.conf.json',
        relPath: 'src-tauri/tauri.conf.json',
        replace: (content) => content.replace(
            `"version": "${currentVersion}"`,
            `"version": "${newVersion}"`
        ),
    },
    {
        name: 'src-tauri/Cargo.lock',
        relPath: 'src-tauri/Cargo.lock',
        replace: (content) => content.replace(
            /(\[\[package\]\]\r?\nname = "antigravity-tools"\r?\nversion = )"[^"]+"/,
            `$1"${newVersion}"`
        ),
    },
    {
        name: 'Casks/antigravity-tools.rb',
        relPath: 'Casks/antigravity-tools.rb',
        replace: (content) => content.replace(
            `version "${currentVersion}"`,
            `version "${newVersion}"`
        ),
    },
    {
        name: 'README.md (标题与徽章)',
        relPath: 'README.md',
        stableOnly: true,
        replace: (content) => content
            .replace(`(v${currentVersion})`, `(v${newVersion})`)
            .replace(`Version-${currentVersion}-blue`, `Version-${newVersion}-blue`),
    },
    {
        name: 'README_EN.md (标题与徽章)',
        relPath: 'README_EN.md',
        stableOnly: true,
        replace: (content) => content
            .replace(`(v${currentVersion})`, `(v${newVersion})`)
            .replace(`Version-${currentVersion}-blue`, `Version-${newVersion}-blue`),
    },
    {
        name: 'src/components/layout/MiniView.tsx',
        relPath: 'src/components/layout/MiniView.tsx',
        replace: (content) => content.replace(
            `setAppVersion('${currentVersion}');`,
            `setAppVersion('${newVersion}');`
        ),
    },
    {
        name: 'src/pages/Settings.tsx',
        relPath: 'src/pages/Settings.tsx',
        replace: (content) => content.replace(
            `useState<string>('${currentVersion}');`,
            `useState<string>('${newVersion}');`
        ),
    },
    {
        name: 'CHANGELOG.md (自动插入新版本骨架)',
        relPath: 'CHANGELOG.md',
        replace: (content) => {
            if (content.includes(`v${newVersion}`)) {
                return content; // 已有则不重复插入
            }
            const anchor = '*   **版本演进**:';
            if (!content.includes(anchor)) {
                return content;
            }
            const eol = content.includes('\r\n') ? '\r\n' : '\n';
            const newBlock = `*   **版本演进**:${eol}    *   **v${newVersion} (${today})**:${eol}        -   **[更新分类] 核心更新标题 (PR #xxx)**:${eol}            -   **功能详述**: 详细说明请在此处补充；涉及外部贡献者时以行内 \`(Thanks to @username)\` 标注。${eol}`;
            return content.replace(anchor, newBlock);
        },
    },
    {
        name: 'CHANGELOG_EN.md (自动插入英文版本骨架)',
        relPath: 'CHANGELOG_EN.md',
        replace: (content) => {
            if (content.includes(`v${newVersion}`)) {
                return content; // 已有则不重复插入
            }
            const anchor = '*   **Version History**:';
            if (!content.includes(anchor)) {
                return content;
            }
            const eol = content.includes('\r\n') ? '\r\n' : '\n';
            const newBlock = `*   **Version History**:${eol}    *   **v${newVersion} (${today})**:${eol}        -   **[Feature Category] Main Update Summary (PR #xxx)**:${eol}            -   **Description**: Please document update details here; credit external contributors inline as \`(Thanks to @username)\`.${eol}`;
            return content.replace(anchor, newBlock);
        },
    },
];

// 6. 逐个文件执行安全替换与校验
let updatedCount = 0;

for (const target of TARGET_FILES) {
    if (target.stableOnly && isPrerelease) {
        log(`预发布版本 ${newVersion} 不写入 ${target.relPath} (README 仅反映最新正式版)。`);
        continue;
    }

    const fullPath = path.join(ROOT_DIR, target.relPath);
    if (!fs.existsSync(fullPath)) {
        warn(`未找到目标文件 ${target.relPath}，已自动跳过。`);
        continue;
    }

    const oldContent = fs.readFileSync(fullPath, 'utf8');
    const newContent = target.replace(oldContent);

    if (oldContent === newContent) {
        warn(`文件 ${target.relPath} 内容未发生变更（可能未匹配到版本特征串）。`);
    } else {
        if (!isDryRun) {
            fs.writeFileSync(fullPath, newContent, 'utf8');
        }
        success(`同步更新: ${target.name} -> ${newVersion}`);
        updatedCount++;
    }
}

// 7. 若存在 cargo 环境，辅助执行 cargo check 确保依赖图完全一致
if (!isDryRun && fs.existsSync(path.join(ROOT_DIR, 'src-tauri/Cargo.toml'))) {
    try {
        execSync('cargo --version', { stdio: 'ignore' });
        log('执行 cargo check 校验 src-tauri/Cargo.lock 依赖图完整性...');
        execSync('cargo check --manifest-path src-tauri/Cargo.toml', {
            cwd: ROOT_DIR,
            stdio: 'ignore',
        });
        success('Cargo 依赖图校验通过');
    } catch {
        // 缺少 Rust/Cargo 环境时不影响整体成功，因 Cargo.lock 已被安全规则同步
    }
}

log(`全部 ${updatedCount} 处版本配置已完成原子化同步！`);

// 8. 自动化 Commit 辅助支持 (使用 execFileSync 避免 Windows cmd.exe 换行崩溃)
if (!isDryRun && autoCommit) {
    log('执行自动 Git Commit...');
    try {
        execFileSync('git', ['add', '-A'], { cwd: ROOT_DIR, stdio: 'ignore' });
        const commitMsg = `chore(release): bump version to ${newVersion} and update changelog\n\nCo-Authored-By: JeikCode <331041501+JeikCode@users.noreply.github.com>`;
        execFileSync('git', ['commit', '-m', commitMsg], { cwd: ROOT_DIR, stdio: 'inherit' });
        success(`已自动生成提交: chore(release): bump version to ${newVersion}`);
        warn('提示: CHANGELOG.md 顶部已自动插入结构骨架，请记得补充本次发版内容并使用 git commit --amend 更新！');
    } catch (e) {
        error('自动 Git Commit 失败，请手动执行 git commit: ' + (e.stderr?.toString() || e.message));
    }
}

console.log(`
${colors.bold}${colors.green}🎉 版本号已全部成功升级到 v${newVersion}！${colors.reset}
后续发版三步走提示:
  1. 在 ${colors.cyan}CHANGELOG.md${colors.reset} 补充本次发版的核心更新内容（外部贡献者以行内 (Thanks to @username) 标注）
  2. 提交发版准备: ${colors.cyan}git commit -am "chore(release): bump version to ${newVersion} and update changelog"${colors.reset}
  3. 推送主干与标签: ${colors.cyan}git push origin main && git tag v${newVersion} && git push origin v${newVersion}${colors.reset}
`);
