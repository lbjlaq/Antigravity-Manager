// File: src/pages/api-proxy/ui/ApiProxyPage.tsx
// Main API Proxy page component

import { useApiProxy } from '../model';
import { LoadingSpinner, ErrorState } from './LoadingState';
import { ProxyConfigCard } from './ProxyConfigCard';
import { ExternalProvidersSection } from './ExternalProvidersSection';
import { ModelRoutingCard } from './ModelRoutingCard';
import { MultiProtocolCard } from './MultiProtocolCard';
import { ModelsTableCard } from './ModelsTableCard';
import { ProxyDialogs } from './ProxyDialogs';

export function ApiProxyPage() {
    const proxy = useApiProxy();

    return (
        <div className="h-full w-full overflow-y-auto overflow-x-hidden">
            <div className="p-5 space-y-4 max-w-7xl mx-auto">

                {/* Loading State */}
                {proxy.configLoading && <LoadingSpinner />}

                {/* Error State */}
                {!proxy.configLoading && proxy.configError && (
                    <ErrorState error={proxy.configError} onRetry={proxy.loadConfig} />
                )}

                {/* Main Content */}
                {!proxy.configLoading && !proxy.configError && proxy.appConfig && (
                    <>
                        {/* Proxy Config Card */}
                        <ProxyConfigCard
                            appConfig={proxy.appConfig}
                            status={proxy.status}
                            loading={proxy.loading}
                            copied={proxy.copied}
                            isEditingApiKey={proxy.isEditingApiKey}
                            tempApiKey={proxy.tempApiKey}
                            isEditingAdminPassword={proxy.isEditingAdminPassword}
                            tempAdminPassword={proxy.tempAdminPassword}
                            onToggle={proxy.handleToggle}
                            onUpdateProxyConfig={proxy.updateProxyConfig}
                            onEditApiKey={proxy.handleEditApiKey}
                            onSaveApiKey={proxy.handleSaveApiKey}
                            onCancelEditApiKey={proxy.handleCancelEditApiKey}
                            onGenerateApiKey={() => proxy.setIsRegenerateKeyConfirmOpen(true)}
                            onEditAdminPassword={proxy.handleEditAdminPassword}
                            onSaveAdminPassword={proxy.handleSaveAdminPassword}
                            onCancelEditAdminPassword={proxy.handleCancelEditAdminPassword}
                            onCopy={proxy.copyToClipboardHandler}
                            setTempApiKey={proxy.setTempApiKey}
                            setTempAdminPassword={proxy.setTempAdminPassword}
                        />

                        {/* External Providers Section */}
                        <ExternalProvidersSection
                            appConfig={proxy.appConfig}
                            status={proxy.status}
                            cfStatus={proxy.cfStatus}
                            cfLoading={proxy.cfLoading}
                            cfMode={proxy.cfMode}
                            cfToken={proxy.cfToken}
                            cfUseHttp2={proxy.cfUseHttp2}
                            copied={proxy.copied}
                            zaiModelOptions={proxy.zaiModelOptions}
                            zaiModelMapping={proxy.zaiModelMapping}
                            zaiModelsLoading={proxy.zaiModelsLoading}
                            zaiNewMappingFrom={proxy.zaiNewMappingFrom}
                            zaiNewMappingTo={proxy.zaiNewMappingTo}
                            setCfMode={proxy.setCfMode}
                            setCfToken={proxy.setCfToken}
                            setCfUseHttp2={proxy.setCfUseHttp2}
                            setZaiNewMappingFrom={proxy.setZaiNewMappingFrom}
                            setZaiNewMappingTo={proxy.setZaiNewMappingTo}
                            updateSchedulingConfig={proxy.updateSchedulingConfig}
                            updateExperimentalConfig={proxy.updateExperimentalConfig}
                            updateCircuitBreakerConfig={proxy.updateCircuitBreakerConfig}
                            updateZaiGeneralConfig={proxy.updateZaiGeneralConfig}
                            updateZaiDefaultModels={proxy.updateZaiDefaultModels}
                            upsertZaiModelMapping={proxy.upsertZaiModelMapping}
                            removeZaiModelMapping={proxy.removeZaiModelMapping}
                            refreshZaiModels={proxy.refreshZaiModels}
                            handleCfInstall={proxy.handleCfInstall}
                            handleCfToggle={proxy.handleCfToggle}
                            handleCfCopyUrl={proxy.handleCfCopyUrl}
                            onClearSessionBindings={() => proxy.setIsClearBindingsConfirmOpen(true)}
                            onClearRateLimits={() => proxy.setIsClearRateLimitsConfirmOpen(true)}
                        />

                        {/* Model Routing Card */}
                        <ModelRoutingCard
                            appConfig={proxy.appConfig}
                            customMappingOptions={proxy.customMappingOptions}
                            customMappingValue={proxy.customMappingValue}
                            editingKey={proxy.editingKey}
                            editingValue={proxy.editingValue}
                            onMappingUpdate={proxy.handleMappingUpdate}
                            onRemoveCustomMapping={proxy.handleRemoveCustomMapping}
                            onApplyPresets={proxy.handleApplyPresets}
                            onResetMapping={() => proxy.setIsResetConfirmOpen(true)}
                            setCustomMappingValue={proxy.setCustomMappingValue}
                            setEditingKey={proxy.setEditingKey}
                            setEditingValue={proxy.setEditingValue}
                        />

                        {/* Multi-Protocol Card */}
                        <MultiProtocolCard
                            appConfig={proxy.appConfig}
                            status={proxy.status}
                            selectedProtocol={proxy.selectedProtocol}
                            copied={proxy.copied}
                            onSelectProtocol={proxy.setSelectedProtocol}
                            onCopy={proxy.copyToClipboardHandler}
                        />

                        {/* Models Table Card */}
                        <ModelsTableCard
                            models={proxy.filteredModels}
                            selectedModelId={proxy.selectedModelId}
                            selectedProtocol={proxy.selectedProtocol}
                            copied={proxy.copied}
                            onSelectModel={proxy.setSelectedModelId}
                            onCopy={proxy.copyToClipboardHandler}
                            getPythonExample={proxy.getPythonExample}
                        />
                    </>
                )}

                {/* Dialogs */}
                <ProxyDialogs
                    isResetConfirmOpen={proxy.isResetConfirmOpen}
                    isRegenerateKeyConfirmOpen={proxy.isRegenerateKeyConfirmOpen}
                    isClearBindingsConfirmOpen={proxy.isClearBindingsConfirmOpen}
                    isClearRateLimitsConfirmOpen={proxy.isClearRateLimitsConfirmOpen}
                    onResetConfirm={proxy.executeResetMapping}
                    onResetCancel={() => proxy.setIsResetConfirmOpen(false)}
                    onRegenerateKeyConfirm={proxy.executeGenerateApiKey}
                    onRegenerateKeyCancel={() => proxy.setIsRegenerateKeyConfirmOpen(false)}
                    onClearBindingsConfirm={proxy.executeClearSessionBindings}
                    onClearBindingsCancel={() => proxy.setIsClearBindingsConfirmOpen(false)}
                    onClearRateLimitsConfirm={proxy.executeClearRateLimits}
                    onClearRateLimitsCancel={() => proxy.setIsClearRateLimitsConfirmOpen(false)}
                />
            </div>
        </div>
    );
}
