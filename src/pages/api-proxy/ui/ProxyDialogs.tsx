// File: src/pages/api-proxy/ui/ProxyDialogs.tsx
// Modal dialogs for API Proxy page

import { useTranslation } from 'react-i18next';
import { ModalDialog } from '@/shared/ui';

interface ProxyDialogsProps {
    isResetConfirmOpen: boolean;
    isRegenerateKeyConfirmOpen: boolean;
    isClearBindingsConfirmOpen: boolean;
    isClearRateLimitsConfirmOpen: boolean;
    onResetConfirm: () => void;
    onResetCancel: () => void;
    onRegenerateKeyConfirm: () => void;
    onRegenerateKeyCancel: () => void;
    onClearBindingsConfirm: () => void;
    onClearBindingsCancel: () => void;
    onClearRateLimitsConfirm: () => void;
    onClearRateLimitsCancel: () => void;
}

export function ProxyDialogs({
    isResetConfirmOpen,
    isRegenerateKeyConfirmOpen,
    isClearBindingsConfirmOpen,
    isClearRateLimitsConfirmOpen,
    onResetConfirm,
    onResetCancel,
    onRegenerateKeyConfirm,
    onRegenerateKeyCancel,
    onClearBindingsConfirm,
    onClearBindingsCancel,
    onClearRateLimitsConfirm,
    onClearRateLimitsCancel,
}: ProxyDialogsProps) {
    const { t } = useTranslation();

    return (
        <>
            <ModalDialog
                isOpen={isResetConfirmOpen}
                title={t('proxy.dialog.reset_mapping_title') || 'Reset Mapping'}
                message={t('proxy.dialog.reset_mapping_msg') || 'Are you sure you want to reset all model mappings to system defaults?'}
                type="confirm"
                isDestructive={true}
                onConfirm={onResetConfirm}
                onCancel={onResetCancel}
            />

            <ModalDialog
                isOpen={isRegenerateKeyConfirmOpen}
                title={t('proxy.dialog.regenerate_key_title') || t('proxy.dialog.confirm_regenerate')}
                message={t('proxy.dialog.regenerate_key_msg') || t('proxy.dialog.confirm_regenerate')}
                type="confirm"
                isDestructive={true}
                onConfirm={onRegenerateKeyConfirm}
                onCancel={onRegenerateKeyCancel}
            />

            <ModalDialog
                isOpen={isClearBindingsConfirmOpen}
                title={t('proxy.dialog.clear_bindings_title') || 'Clear Session Bindings'}
                message={t('proxy.dialog.clear_bindings_msg') || 'Are you sure you want to clear all session-to-account binding mappings?'}
                type="confirm"
                isDestructive={true}
                onConfirm={onClearBindingsConfirm}
                onCancel={onClearBindingsCancel}
            />

            <ModalDialog
                isOpen={isClearRateLimitsConfirmOpen}
                title={t('proxy.dialog.clear_rate_limits_title') || 'Clear Rate Limits'}
                message={t('proxy.dialog.clear_rate_limits_confirm') || 'Are you sure you want to clear all local rate limit records?'}
                type="confirm"
                isDestructive={true}
                onConfirm={onClearRateLimitsConfirm}
                onCancel={onClearRateLimitsCancel}
            />
        </>
    );
}
