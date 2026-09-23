<!--
一类问题一个 PR：请让本 PR 只解决同一类问题；PR 内的每个提交都应能单独 revert 而不影响其它提交。
English PRs are welcome — the same fields apply.
-->

## 解决什么问题

<!-- 一句话说明这一类问题。关联 issue / PR 请用关键字：Closes #N / Fixes #N -->

## 变更类型

- [ ] 修复（bug / 回归）
- [ ] 功能
- [ ] 文档 / 规范 / 发版基建（不夹带进功能 PR；同类文档改动合并为一个 PR）

## 提交清单（组内独立、可单独回退）

| 提交 | 作用 | 可否单独 revert |
| --- | --- | --- |
|  |  |  |

## 行为变化与影响面

<!-- 对**原本正常工作**的输入是否产生行为变化？如有，请写清前后对比。
     例：DSH 身份串 `You are an AI agent powered by DeepSeek Harness, …` 会被归一化为中性身份 -->

## 已知未验证项

<!-- 哪些路径没有实测过（缺环境 / 缺样本）？只标注事实，不要给结论 -->

## 如何回退

<!-- 出问题时怎么快速回退：revert 哪条提交 / 关哪个开关 / 是否需要数据修复 -->

## 自检（与 CI 相同的命令）

- [ ] `cd src-tauri && cargo fmt -- --check`
- [ ] `cd src-tauri && cargo clippy --all-targets --all-features`
- [ ] `cd src-tauri && cargo check`
- [ ] `npm run build`
- [ ] 改动涉及的模块已在本地运行单测（`cargo test` 不在 CI 门禁内，不做全量跑测）

## 署名

<!-- 外部贡献：作者 = PR 开启者，squash 合并后仍归其名下。
     若由维护者代为落地，请在此说明并补 Co-authored-by。 -->
