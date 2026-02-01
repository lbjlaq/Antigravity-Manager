// File: src/app/providers/I18nProvider.tsx
// I18n initialization provider

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import '@/shared/config/i18n'; // Initialize i18n

interface I18nProviderProps {
  children: React.ReactNode;
  language?: string;
}

export function I18nProvider({ children, language }: I18nProviderProps) {
  const { i18n } = useTranslation();

  useEffect(() => {
    if (language) {
      i18n.changeLanguage(language);
      // Support RTL languages
      document.documentElement.dir = language === 'ar' ? 'rtl' : 'ltr';
    }
  }, [language, i18n]);

  return <>{children}</>;
}
