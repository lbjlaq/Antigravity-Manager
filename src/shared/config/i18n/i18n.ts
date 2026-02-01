// File: src/shared/config/i18n/i18n.ts
// i18n configuration

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import en from "./locales/en.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";
import ja from "./locales/ja.json";
import tr from "./locales/tr.json";
import vi from "./locales/vi.json";
import pt from "./locales/pt.json";
import ru from "./locales/ru.json";
import ko from "./locales/ko.json";
import ar from "./locales/ar.json";

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
      ja: { translation: ja },
      tr: { translation: tr },
      "zh-CN": { translation: zh },
      vi: { translation: vi },
      "vi-VN": { translation: vi },
      pt: { translation: pt },
      "pt-BR": { translation: pt },
      ru: { translation: ru },
      ko: { translation: ko },
      ar: { translation: ar },
    },
    fallbackLng: "en",
    debug: false,
    interpolation: {
      escapeValue: false,
    },
  });

export default i18n;
