export type PedalTargetModel = 'gemini' | 'claude';

export interface PedalCandidate {
    accountId: string;
    accountName: string;
    email: string;
    model: PedalTargetModel;
    percentage: number;
    resetTime: string;
    hoursLeft: number;
}

export interface PedalModeState {
    active: boolean;
    targetModel: PedalTargetModel | null;
    targetAccountIds: string[];
    snapshotDisabledIds: string[];
    temporarilyDisabledIds: string[];
    suppressNotice: {
        gemini: boolean;
        claude: boolean;
    };
}
