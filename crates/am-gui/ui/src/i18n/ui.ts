import zhPack from './packs/zh.json';
import enPack from './packs/en.json';

type UiDictionary = Record<string, string>;

const dictionaries: Record<string, UiDictionary> = {
  zh: zhPack,
  'zh-CN': zhPack,
  en: enPack,
};

const packLoaders: Record<string, () => Promise<{ default: UiDictionary }>> = {
  'zh-TW': () => import('./packs/zh-TW.json'),
  ja: () => import('./packs/ja.json'),
  ko: () => import('./packs/ko.json'),
  ru: () => import('./packs/ru.json'),
  es: () => import('./packs/es.json'),
  pt: () => import('./packs/pt.json'),
  tr: () => import('./packs/tr.json'),
  vi: () => import('./packs/vi.json'),
  ar: () => import('./packs/ar.json'),
  my: () => import('./packs/my.json'),
};

const loadingPromises = new Map<string, Promise<void>>();

/**
 * 按需异步加载非默认 UI 语言包（仅 zh/en 共 ~39KB 同步常驻），
 * 避免 12 份语言全量进入 JS 堆内存。
 */
export function ensureUiLocaleLoaded(lang: string): Promise<void> {
  if (dictionaries[lang]) {
    return Promise.resolve();
  }
  const loader = packLoaders[lang];
  if (!loader) {
    return Promise.resolve();
  }
  const existing = loadingPromises.get(lang);
  if (existing) {
    return existing;
  }
  const promise = loader()
    .then((mod) => {
      dictionaries[lang] = mod.default;
    })
    .catch(() => {
      // 回退到 en / zh
    })
    .finally(() => {
      loadingPromises.delete(lang);
    });
  loadingPromises.set(lang, promise);
  return promise;
}

/**
 * 快速翻译助手，支持 key 查找与占位符变量替换
 */
export function tUi(lang: string, key: string, params?: Record<string, string | number>): string {
  if (!dictionaries[lang] && packLoaders[lang]) {
    void ensureUiLocaleLoaded(lang);
  }

  let text = dictionaries[lang]?.[key];
  if (!text) {
    if (lang === 'zh-TW') {
      text = dictionaries['zh-TW']?.[key] || dictionaries.zh[key];
    } else {
      text = dictionaries.en[key] || dictionaries.zh[key] || key;
    }
  }

  if (params && text) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{\\{${k}\\}\\}`, 'g'), String(v));
    }
  }

  return text;
}
