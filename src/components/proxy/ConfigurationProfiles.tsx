import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
    Zap,
    Sparkles,
    Brain,
    Palette,
    CheckCircle,
    Clock,
    Gauge,
    Target
} from 'lucide-react';

interface ThinkingConfig {
    enabled: boolean;
    budgetTokens?: number;
}

interface Profile {
    id: string;
    name: string;
    description: string;
    model: string;
    thinking: ThinkingConfig;
    maxTokens: number;
    temperature: number;
    useCase: string[];
    icon: React.ReactNode;
    color: string;
}

// 8 Profile Presets (4 base + 4 thinking)
const PROFILES: Profile[] = [
    // Base Profiles (Thinking Disabled)
    {
        id: 'fast-general',
        name: 'Fast General',
        description: 'Quick responses for general tasks',
        model: 'gemini-3-pro-high',
        thinking: { enabled: false },
        maxTokens: 8192,
        temperature: 0.7,
        useCase: ['chat', 'general', 'quick'],
        icon: <Zap size={20} />,
        color: 'bg-yellow-500'
    },
    {
        id: 'balanced',
        name: 'Balanced',
        description: 'Balance between speed and quality',
        model: 'gemini-3-pro-high',
        thinking: { enabled: false },
        maxTokens: 16384,
        temperature: 0.7,
        useCase: ['coding', 'analysis', 'balanced'],
        icon: <Gauge size={20} />,
        color: 'bg-blue-500'
    },
    {
        id: 'quality',
        name: 'High Quality',
        description: 'Maximum quality responses',
        model: 'gemini-3-pro-high',
        thinking: { enabled: false },
        maxTokens: 32000,
        temperature: 0.5,
        useCase: ['complex', 'detailed', 'quality'],
        icon: <Target size={20} />,
        color: 'bg-green-500'
    },
    {
        id: 'creative',
        name: 'Creative',
        description: 'Creative and diverse responses',
        model: 'gemini-3-pro-high',
        thinking: { enabled: false },
        maxTokens: 16384,
        temperature: 1.0,
        useCase: ['creative', 'brainstorming', 'writing'],
        icon: <Palette size={20} />,
        color: 'bg-purple-500'
    },

    // Thinking Profiles (4 variants with thinking enabled)
    {
        id: 'thinking-fast',
        name: 'Thinking Fast',
        description: 'Quick reasoning with small thinking budget',
        model: 'gemini-3-pro-high',
        thinking: { enabled: true, budgetTokens: 4096 },
        maxTokens: 16384,
        temperature: 0.7,
        useCase: ['reasoning', 'problem-solving', 'fast'],
        icon: <Brain size={20} />,
        color: 'bg-indigo-500'
    },
    {
        id: 'thinking-balanced',
        name: 'Thinking Balanced',
        description: 'Balanced reasoning with moderate budget',
        model: 'gemini-3-pro-high',
        thinking: { enabled: true, budgetTokens: 8192 },
        maxTokens: 24576,
        temperature: 0.7,
        useCase: ['analysis', 'research', 'balanced'],
        icon: <Brain size={20} />,
        color: 'bg-blue-600'
    },
    {
        id: 'thinking-deep',
        name: 'Thinking Deep',
        description: 'Deep reasoning with large thinking budget',
        model: 'gemini-3-pro-high',
        thinking: { enabled: true, budgetTokens: 16384 },
        maxTokens: 32768,
        temperature: 0.5,
        useCase: ['complex', 'deep-analysis', 'research'],
        icon: <Sparkles size={20} />,
        color: 'bg-purple-600'
    },
    {
        id: 'thinking-max',
        name: 'Thinking Maximum',
        description: 'Maximum reasoning capability',
        model: 'gemini-3-pro-high',
        thinking: { enabled: true, budgetTokens: 32768 },
        maxTokens: 65536,
        temperature: 0.5,
        useCase: ['expert', 'critical', 'maximum'],
        icon: <Sparkles size={20} />,
        color: 'bg-pink-600'
    }
];

interface ProfileCardProps {
    profile: Profile;
    onSelect: (profile: Profile) => void;
    isActive: boolean;
}

const ProfileCard: React.FC<ProfileCardProps> = ({ profile, onSelect, isActive }) => {
    const { t } = useTranslation();

    return (
        <div
            className={`
                card bg-white dark:bg-base-100 shadow-sm border-2 transition-all cursor-pointer
                hover:shadow-lg hover:scale-[1.02] relative overflow-hidden
                ${isActive
                    ? 'border-blue-500 ring-2 ring-blue-200 dark:ring-blue-800'
                    : 'border-gray-200 dark:border-gray-700 hover:border-blue-300'
                }
            `}
            onClick={() => onSelect(profile)}
        >
            {/* Active indicator */}
            {isActive && (
                <div className="absolute top-3 right-3 z-10">
                    <CheckCircle size={20} className="text-blue-500 fill-blue-100 dark:fill-blue-900" />
                </div>
            )}

            <div className="card-body p-4">
                {/* Icon + Title */}
                <div className="flex items-start gap-3 mb-3">
                    <div className={`w-10 h-10 rounded-lg ${profile.color} flex items-center justify-center text-white shadow-md shrink-0`}>
                        {profile.icon}
                    </div>
                    <div className="flex-1 min-w-0">
                        <h3 className="card-title text-sm font-bold text-gray-900 dark:text-gray-100 mb-1">
                            {t(`profiles.${profile.id}.name`, profile.name)}
                        </h3>
                        <p className="text-xs text-gray-600 dark:text-gray-400 line-clamp-2">
                            {t(`profiles.${profile.id}.description`, profile.description)}
                        </p>
                    </div>
                </div>

                {/* Badges */}
                <div className="flex flex-wrap gap-1.5 mb-3">
                    {profile.thinking.enabled && (
                        <span className="badge badge-sm bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300 border-purple-200 dark:border-purple-800">
                            <Brain size={12} className="mr-1" />
                            {t('profiles.thinking_enabled')}
                        </span>
                    )}
                    <span className="badge badge-sm bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300">
                        {profile.maxTokens.toLocaleString()} tokens
                    </span>
                    <span className="badge badge-sm bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300">
                        T: {profile.temperature}
                    </span>
                </div>

                {/* Use Cases */}
                <div className="flex flex-wrap gap-1 mb-4">
                    {profile.useCase.slice(0, 3).map((useCase) => (
                        <span
                            key={useCase}
                            className="text-[10px] px-2 py-0.5 rounded-full bg-blue-50 text-blue-600 dark:bg-blue-900/20 dark:text-blue-400"
                        >
                            {t(`profiles.use_case.${useCase}`, useCase)}
                        </span>
                    ))}
                </div>

                {/* Stats */}
                <div className="grid grid-cols-2 gap-2 pt-3 border-t border-gray-100 dark:border-gray-700">
                    <div className="text-xs">
                        <div className="text-gray-500 dark:text-gray-400 mb-0.5">
                            {t('profiles.max_tokens')}
                        </div>
                        <div className="font-mono font-bold text-gray-900 dark:text-gray-100">
                            {(profile.maxTokens / 1024).toFixed(0)}K
                        </div>
                    </div>
                    {profile.thinking.enabled && profile.thinking.budgetTokens && (
                        <div className="text-xs">
                            <div className="text-gray-500 dark:text-gray-400 mb-0.5">
                                {t('profiles.thinking_budget')}
                            </div>
                            <div className="font-mono font-bold text-purple-600 dark:text-purple-400">
                                {(profile.thinking.budgetTokens / 1024).toFixed(0)}K
                            </div>
                        </div>
                    )}
                    {!profile.thinking.enabled && (
                        <div className="text-xs">
                            <div className="text-gray-500 dark:text-gray-400 mb-0.5">
                                {t('profiles.response_time')}
                            </div>
                            <div className="font-bold text-green-600 dark:text-green-400 flex items-center gap-1">
                                <Clock size={12} />
                                {profile.maxTokens <= 8192 ? t('profiles.speed.fast') :
                                 profile.maxTokens <= 16384 ? t('profiles.speed.medium') :
                                 t('profiles.speed.slow')}
                            </div>
                        </div>
                    )}
                </div>

                {/* Apply Button */}
                <button
                    className={`
                        btn btn-sm w-full mt-3 transition-all
                        ${isActive
                            ? 'btn-success text-white'
                            : 'btn-primary'
                        }
                    `}
                    onClick={(e) => {
                        e.stopPropagation();
                        onSelect(profile);
                    }}
                >
                    {isActive ? (
                        <>
                            <CheckCircle size={14} />
                            {t('profiles.active')}
                        </>
                    ) : (
                        t('profiles.apply_profile')
                    )}
                </button>
            </div>
        </div>
    );
};

interface ConfigurationProfilesProps {
    currentModel?: string;
    currentThinking?: ThinkingConfig;
    currentMaxTokens?: number;
    currentTemperature?: number;
    onProfileSelect?: (profile: Profile) => void;
}

export const ConfigurationProfiles: React.FC<ConfigurationProfilesProps> = ({
    currentModel,
    currentThinking,
    currentMaxTokens,
    currentTemperature,
    onProfileSelect
}) => {
    const { t } = useTranslation();
    const [selectedCategory, setSelectedCategory] = useState<'all' | 'base' | 'thinking'>('all');

    const isProfileActive = (profile: Profile): boolean => {
        if (!currentModel) return false;

        return (
            profile.model === currentModel &&
            profile.thinking.enabled === currentThinking?.enabled &&
            profile.maxTokens === currentMaxTokens &&
            profile.temperature === currentTemperature &&
            (!profile.thinking.enabled ||
             profile.thinking.budgetTokens === currentThinking?.budgetTokens)
        );
    };

    const handleSelectProfile = (profile: Profile) => {
        if (onProfileSelect) {
            onProfileSelect(profile);
        }
    };

    const filteredProfiles = PROFILES.filter(profile => {
        if (selectedCategory === 'all') return true;
        if (selectedCategory === 'base') return !profile.thinking.enabled;
        if (selectedCategory === 'thinking') return profile.thinking.enabled;
        return true;
    });

    return (
        <div className="profiles-container space-y-4">
            {/* Header */}
            <div className="flex items-center justify-between">
                <div>
                    <h2 className="text-lg font-bold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                        <Sparkles size={20} className="text-blue-500" />
                        {t('profiles.title')}
                    </h2>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                        {t('profiles.subtitle')}
                    </p>
                </div>

                {/* Category Filter */}
                <div className="flex gap-2">
                    <button
                        className={`btn btn-xs ${selectedCategory === 'all' ? 'btn-primary' : 'btn-ghost'}`}
                        onClick={() => setSelectedCategory('all')}
                    >
                        {t('profiles.filter.all')} ({PROFILES.length})
                    </button>
                    <button
                        className={`btn btn-xs ${selectedCategory === 'base' ? 'btn-primary' : 'btn-ghost'}`}
                        onClick={() => setSelectedCategory('base')}
                    >
                        {t('profiles.filter.base')} (4)
                    </button>
                    <button
                        className={`btn btn-xs ${selectedCategory === 'thinking' ? 'btn-primary' : 'btn-ghost'}`}
                        onClick={() => setSelectedCategory('thinking')}
                    >
                        <Brain size={12} />
                        {t('profiles.filter.thinking')} (4)
                    </button>
                </div>
            </div>

            {/* Profiles Grid */}
            <div className="profiles-grid grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                {filteredProfiles.map(profile => (
                    <ProfileCard
                        key={profile.id}
                        profile={profile}
                        onSelect={handleSelectProfile}
                        isActive={isProfileActive(profile)}
                    />
                ))}
            </div>

            {/* Info Banner */}
            <div className="bg-blue-50 dark:bg-blue-900/10 border border-blue-200 dark:border-blue-800 rounded-lg p-3">
                <div className="flex items-start gap-2">
                    <Sparkles size={16} className="text-blue-500 shrink-0 mt-0.5" />
                    <div className="text-xs text-blue-700 dark:text-blue-300">
                        <strong>{t('profiles.info.title')}:</strong> {t('profiles.info.description')}
                    </div>
                </div>
            </div>
        </div>
    );
};

export default ConfigurationProfiles;
