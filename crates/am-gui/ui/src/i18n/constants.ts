export interface Language {
  code: string;
  label: string;
  short: string;
}

export const LANGUAGES: Language[] = [
  { code: 'zh', label: '简体中文', short: 'ZH' },
  { code: 'zh-TW', label: '繁體中文', short: 'TW' },
  { code: 'en', label: 'English', short: 'EN' },
  { code: 'ja', label: '日本語', short: 'JA' },
  { code: 'tr', label: 'Türkçe', short: 'TR' },
  { code: 'vi', label: 'Tiếng Việt', short: 'VI' },
  { code: 'pt', label: 'Português', short: 'PT' },
  { code: 'ko', label: '한국어', short: 'KO' },
  { code: 'ru', label: 'Русский', short: 'RU' },
  { code: 'ar', label: 'العربية', short: 'AR' },
  { code: 'es', label: 'Español', short: 'ES' },
  { code: 'my', label: 'Bahasa Melayu', short: 'MY' },
];

export type LangCode =
  | 'zh'
  | 'zh-TW'
  | 'en'
  | 'ja'
  | 'tr'
  | 'vi'
  | 'pt'
  | 'ko'
  | 'ru'
  | 'ar'
  | 'es'
  | 'my';
