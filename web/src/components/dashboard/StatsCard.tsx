import { LucideIcon } from 'lucide-react';

interface StatsCardProps {
    icon: LucideIcon;
    title: string;
    value: string | number;
    description?: string;
    colorClass?: string;
}

function StatsCard({ icon: Icon, title, value, description, colorClass = 'primary' }: StatsCardProps) {
    return (
        <div className="stat bg-base-100 shadow rounded-lg">
            <div className={`stat-figure text-${colorClass}`}>
                <Icon className="w-8 h-8" />
            </div>
            <div className="stat-title">{title}</div>
            <div className={`stat-value text-${colorClass}`}>{value}</div>
            {description && <div className="stat-desc">{description}</div>}
        </div>
    );
}

export default StatsCard;
