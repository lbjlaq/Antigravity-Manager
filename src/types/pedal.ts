import type { ModelPriorities } from './account';

export type PedalTargetModel = 'gemini' | 'claude';

export interface PedalCandidate {
    accountId: string;
    accountName: string;
    email: string;
    model: PedalTargetModel;
    modelBadge?: string;
    supportedModels?: string[];
    percentage: number;
    resetTime: string;
    hoursLeft: number;
}

export interface PedalModeState {
    active: boolean;
    targetModel: PedalTargetModel | null;
    targetAccountIds: string[];
    originalPriorities: Record<string, number>;
    originalModelPriorities?: Record<string, ModelPriorities>;
    snapshotDisabledIds: string[];
    temporarilyDisabledIds: string[];
    suppressNotice: {
        gemini: boolean;
        claude: boolean;
    };
}
