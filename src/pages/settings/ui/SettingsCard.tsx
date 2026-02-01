// File: src/pages/settings/ui/SettingsCard.tsx
// Unified settings card - matches Accounts/Dashboard style

import { memo } from 'react';
import { cn } from '@/shared/lib';
import type { LucideIcon } from 'lucide-react';

interface SettingsCardProps {
  title: string;
  icon: LucideIcon;
  children: React.ReactNode;
  className?: string;
  description?: string;
}

export const SettingsCard = memo(function SettingsCard({
  title,
  icon: Icon,
  children,
  className,
  description,
}: SettingsCardProps) {
  return (
    <div
      className={cn(
        "rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-5",
        className
      )}
    >
      <div className="flex items-center gap-3 mb-5">
        <div className="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-800">
          <Icon className="h-4 w-4 text-zinc-600 dark:text-zinc-400" />
        </div>
        <div>
          <h3 className="text-sm font-semibold text-zinc-900 dark:text-white">{title}</h3>
          {description && <p className="text-xs text-zinc-500">{description}</p>}
        </div>
      </div>
      <div>
        {children}
      </div>
    </div>
  );
});
