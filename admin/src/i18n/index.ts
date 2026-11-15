import i18n from "i18next";
import { useCallback } from "react";
import { initReactI18next, useTranslation } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import zh from "@/i18n/zh";
import en from "@/i18n/en";

export function useI18n(namespace?: string) {
    const { t } = useTranslation();
    return useCallback(
        (key: string) => {
            if (namespace) {
                return t(`${namespace}.${key}`);
            }
            return t(key);
        },
        [t, namespace],
    );
}

i18n.use(LanguageDetector)
    .use(initReactI18next)
    .init({
        fallbackLng: "en",
        interpolation: {
            escapeValue: false,
        },
        resources: {
            en: {
                translation: en,
            },
            zh: {
                translation: zh,
            },
        },
    });

export default i18n;
