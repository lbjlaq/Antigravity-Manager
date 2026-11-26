import React, { useEffect, useMemo, useState } from 'react';
import { api } from '../api';

interface ReleaseNotesViewProps {
  notes: string;
  lang: string;
  version?: string;
  publishedAt?: string;
  title?: string;
  variant?: 'compact' | 'comfortable';
}

interface ParsedSections {
  zh?: string;
  en?: string;
  plain?: string;
}

interface BulletNode {
  kind: 'bullet';
  title?: string;
  body: string;
  children: string[];
}

interface HeadingNode {
  kind: 'heading';
  text: string;
}

interface ParagraphNode {
  kind: 'paragraph';
  text: string;
}

type NoteBlock = BulletNode | HeadingNode | ParagraphNode;

/**
 * 去掉 GitHub Release 降级回退时顶部的 Edition Guide / Quick Download 提示块（`> [!NOTE] ... ---`）
 */
function stripEditionGuideBanner(raw: string): string {
  const normalized = raw.replace(/\r\n/g, '\n').trim();
  if (!normalized.startsWith('>')) {
    return normalized;
  }
  const sepIdx = normalized.indexOf('\n---');
  if (sepIdx !== -1) {
    const rest = normalized.slice(sepIdx + 4).trim();
    if (rest.length > 0) {
      return rest;
    }
  }
  return normalized;
}

/**
 * 解析双语分段（### English / ### 中文）
 */
function splitBilingualNotes(raw: string): ParsedSections {
  const cleaned = stripEditionGuideBanner(raw);
  const lines = cleaned.split('\n');

  let currentKey: 'en' | 'zh' | null = null;
  const buckets: { en: string[]; zh: string[]; other: string[] } = {
    en: [],
    zh: [],
    other: [],
  };

  for (const line of lines) {
    const headingMatch = line.match(/^#{2,3}\s+(.+?)\s*$/);
    if (headingMatch) {
      const label = headingMatch[1].trim().toLowerCase();
      if (label === 'english' || label === 'en') {
        currentKey = 'en';
        continue;
      }
      if (
        label === '中文' ||
        label === '简体中文' ||
        label === '繁體中文' ||
        label === 'chinese' ||
        label === 'zh'
      ) {
        currentKey = 'zh';
        continue;
      }
    }

    if (currentKey) {
      buckets[currentKey].push(line);
    } else {
      buckets.other.push(line);
    }
  }

  const en = buckets.en.join('\n').trim();
  const zh = buckets.zh.join('\n').trim();
  if (en || zh) {
    return {
      en: en || undefined,
      zh: zh || undefined,
    };
  }
  return { plain: cleaned };
}

/**
 * 提取 `- **小标题**：说明正文` 结构，使标题与正文分层展示
 */
function extractBulletTitle(text: string): { title?: string; body: string } {
  const match = text.match(/^\*\*(.+?)\*\*\s*[:：]\s*([\s\S]*)$/);
  if (match) {
    return {
      title: match[1].trim(),
      body: match[2].trim(),
    };
  }
  return { body: text };
}

/**
 * 将单语种 Markdown 文本解析为结构化块列表
 */
function parseBlocks(markdown: string): NoteBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const blocks: NoteBlock[] = [];

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed === '---') continue;

    // 子级列表项（缩进 2+ 空格）
    const subBullet = rawLine.match(/^\s{2,}[-*]\s+(.+)$/);
    if (subBullet) {
      const last = blocks[blocks.length - 1];
      if (last && last.kind === 'bullet') {
        last.children.push(subBullet[1].trim());
        continue;
      }
    }

    // 一级列表项
    const topBullet = rawLine.match(/^\s*[-*]\s+(.+)$/);
    if (topBullet) {
      const { title, body } = extractBulletTitle(topBullet[1].trim());
      blocks.push({
        kind: 'bullet',
        title,
        body,
        children: [],
      });
      continue;
    }

    // 小标题
    const heading = trimmed.match(/^#{1,4}\s+(.+)$/);
    if (heading) {
      blocks.push({
        kind: 'heading',
        text: heading[1].trim(),
      });
      continue;
    }

    // 普通段落或续行
    const last = blocks[blocks.length - 1];
    if (last && last.kind === 'paragraph') {
      last.text = `${last.text} ${trimmed}`;
    } else {
      blocks.push({
        kind: 'paragraph',
        text: trimmed.replace(/^>\s*/, ''),
      });
    }
  }

  return blocks;
}

/**
 * 安全的行内 Markdown 渲染（支持 `code`、**bold**、[text](https://...)）
 */
function renderInline(text: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const tokenRe = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g;
  let lastIdx = 0;
  let match: RegExpExecArray | null;
  let partIdx = 0;

  while ((match = tokenRe.exec(text)) !== null) {
    if (match.index > lastIdx) {
      nodes.push(text.slice(lastIdx, match.index));
    }

    const token = match[0];
    const key = `${keyPrefix}-${partIdx++}`;

    if (token.startsWith('`') && token.endsWith('`')) {
      nodes.push(
        <code
          key={key}
          className="px-1.5 py-[1px] mx-[1px] rounded-md bg-gray-200/75 dark:bg-white/[0.08] text-gray-800 dark:text-zinc-200 font-mono text-[0.9em] break-all"
        >
          {token.slice(1, -1)}
        </code>
      );
    } else if (token.startsWith('**') && token.endsWith('**')) {
      nodes.push(
        <strong key={key} className="font-semibold text-gray-900 dark:text-zinc-100">
          {renderInline(token.slice(2, -2), `${key}-b`)}
        </strong>
      );
    } else {
      const linkMatch = token.match(/^\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)$/);
      if (linkMatch) {
        const [, label, href] = linkMatch;
        nodes.push(
          <button
            key={key}
            type="button"
            onClick={() => void api.openExternalUrl(href)}
            className="text-blue-600 dark:text-blue-400 hover:underline font-medium cursor-pointer inline"
          >
            {label}
          </button>
        );
      } else {
        nodes.push(token);
      }
    }

    lastIdx = match.index + token.length;
  }

  if (lastIdx < text.length) {
    nodes.push(text.slice(lastIdx));
  }

  return nodes;
}

export const ReleaseNotesView: React.FC<ReleaseNotesViewProps> = ({
  notes,
  lang,
  publishedAt,
  title,
  variant = 'comfortable',
}) => {
  const sections = useMemo(() => splitBilingualNotes(notes), [notes]);
  const hasBoth = Boolean(sections.zh && sections.en);
  const preferredLang: 'zh' | 'en' = lang.toLowerCase().startsWith('zh') ? 'zh' : 'en';
  const [selectedLang, setSelectedLang] = useState<'zh' | 'en'>(preferredLang);

  useEffect(() => {
    setSelectedLang(preferredLang);
  }, [preferredLang]);

  const activeText = useMemo(() => {
    if (sections.plain) return sections.plain;
    if (selectedLang === 'zh') return sections.zh || sections.en || '';
    return sections.en || sections.zh || '';
  }, [sections, selectedLang]);

  const blocks = useMemo(() => parseBlocks(activeText), [activeText]);
  if (blocks.length === 0) return null;

  const isCompact = variant === 'compact';

  return (
    <div
      className={`rounded-xl bg-gray-50/90 dark:bg-white/[0.03] border border-gray-200/70 dark:border-white/[0.07] text-left overflow-hidden ${
        isCompact ? 'mt-3' : 'mt-1'
      }`}
    >
      {/* 顶部栏：标题 + 日期 + 极简中英切换胶囊 */}
      {(title || publishedAt || hasBoth) && (
        <div
          className={`flex items-center justify-between gap-2 border-b border-gray-200/60 dark:border-white/[0.06] ${
            isCompact ? 'px-3 py-1.5' : 'px-3.5 py-2'
          }`}
        >
          <div className="flex items-center gap-2 min-w-0">
            {title && (
              <span className="text-[11px] font-semibold text-gray-800 dark:text-zinc-200 truncate">
                {title}
              </span>
            )}
            {publishedAt && (
              <span className="text-[10px] font-mono text-gray-400 dark:text-zinc-500 shrink-0">
                {publishedAt.slice(0, 10)}
              </span>
            )}
          </div>

          {hasBoth && (
            <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-200/70 dark:bg-white/[0.06] shrink-0">
              <button
                type="button"
                onClick={() => setSelectedLang('zh')}
                className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                  selectedLang === 'zh'
                    ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs'
                    : 'text-gray-500 dark:text-zinc-400 hover:text-gray-700 dark:hover:text-zinc-200'
                }`}
              >
                中文
              </button>
              <button
                type="button"
                onClick={() => setSelectedLang('en')}
                className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                  selectedLang === 'en'
                    ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs'
                    : 'text-gray-500 dark:text-zinc-400 hover:text-gray-700 dark:hover:text-zinc-200'
                }`}
              >
                EN
              </button>
            </div>
          )}
        </div>
      )}

      {/* 结构化正文列表 */}
      <div
        className={`mini-scroll overflow-y-auto ${
          isCompact ? 'px-3 py-2.5 max-h-48 space-y-2.5' : 'px-3.5 py-3 max-h-72 space-y-3'
        }`}
      >
        {blocks.map((block, idx) => {
          if (block.kind === 'heading') {
            return (
              <div
                key={`h-${idx}`}
                className="text-[11px] font-bold text-gray-800 dark:text-zinc-200 pt-1 first:pt-0"
              >
                {renderInline(block.text, `h-${idx}`)}
              </div>
            );
          }

          if (block.kind === 'paragraph') {
            return (
              <p
                key={`p-${idx}`}
                className={`${
                  isCompact ? 'text-[11px]' : 'text-[11.5px]'
                } leading-relaxed text-gray-600 dark:text-zinc-300`}
              >
                {renderInline(block.text, `p-${idx}`)}
              </p>
            );
          }

          return (
            <div key={`b-${idx}`} className="flex items-start gap-2">
              <span className="mt-[7px] w-1.5 h-1.5 rounded-full bg-blue-500/80 dark:bg-blue-400/80 shrink-0" />
              <div className="min-w-0 flex-1 space-y-1">
                {block.title && (
                  <div
                    className={`${
                      isCompact ? 'text-[11.5px]' : 'text-[12px]'
                    } font-semibold text-gray-900 dark:text-zinc-100 leading-snug`}
                  >
                    {renderInline(block.title, `bt-${idx}`)}
                  </div>
                )}
                {block.body && (
                  <p
                    className={`${
                      isCompact ? 'text-[11px]' : 'text-[11.5px]'
                    } leading-relaxed text-gray-600 dark:text-zinc-300`}
                  >
                    {renderInline(block.body, `bb-${idx}`)}
                  </p>
                )}
                {block.children.length > 0 && (
                  <ul className="mt-1.5 pl-2.5 border-l border-gray-200 dark:border-white/10 space-y-1.5">
                    {block.children.map((child, cIdx) => (
                      <li
                        key={`bc-${idx}-${cIdx}`}
                        className={`${
                          isCompact ? 'text-[10.5px]' : 'text-[11px]'
                        } leading-relaxed text-gray-600 dark:text-zinc-300`}
                      >
                        {renderInline(child, `bc-${idx}-${cIdx}`)}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};
