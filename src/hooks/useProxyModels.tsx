import { useTranslation } from 'react-i18next';
import { MODEL_CONFIG } from '../config/modelConfig';

export const useProxyModels = () => {
    const { t } = useTranslation();

    const models = Object.entries(MODEL_CONFIG).map(([id, config]) => ({
        id,
        name: t(config.i18nKey, config.label),
        desc: t(config.i18nDescKey, config.label),
        group: config.group,
        icon: <config.Icon size={16} />
    }));

    return { models };
};
