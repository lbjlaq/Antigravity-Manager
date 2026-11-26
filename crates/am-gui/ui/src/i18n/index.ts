import { LangCode, LANGUAGES } from './constants';
import { ensureUiLocaleLoaded, tUi } from './ui';

export function ensureLocaleLoaded(lang: string): Promise<void> {
  return ensureUiLocaleLoaded(lang);
}

export function getTranslation(lang: string, keyPath: string, defaultValue?: string): string {
  const normalized = keyPath.replace(/\./g, '_');
  const translated = tUi(lang, normalized);
  if (translated !== normalized) {
    return translated;
  }
  return defaultValue !== undefined ? defaultValue : keyPath;
}

export function createTranslator(lang: string) {
  const isZh = lang === 'zh' || lang === 'zh-TW';
  const isEn = lang === 'en';

  return {
    lang,
    isZh,
    isEn,
    t: (keyPath: string, defaultValue?: string): string => {
      return getTranslation(lang, keyPath, defaultValue);
    },
    pick: <T>(options: { zh: T; en: T; [k: string]: T | undefined }): T => {
      if (options[lang] !== undefined) {
        return options[lang]!;
      }
      if (lang === 'zh-TW' && options['zh-TW'] !== undefined) {
        return options['zh-TW']!;
      }
      if (isZh) {
        return options.zh;
      }
      return options.en;
    },
  };
}

export { LANGUAGES };
export type { LangCode };
