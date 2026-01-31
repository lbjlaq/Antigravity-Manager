// File: src/pages/settings/ui/SettingsCard.tsx
import { memo } from 'react';
import { motion } from 'framer-motion';
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
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className={cn(
        "rounded-2xl border border-zinc-200 dark:border-white/5 bg-white/50 dark:bg-zinc-900/30 backdrop-blur-md p-6 shadow-sm hover:shadow-md transition-all duration-300 relative group",
        className
      )}
    >
      <div className="absolute top-0 right-0 w-32 h-32 bg-indigo-500/5 rounded-bl-full -mr-10 -mt-10 transition-transform group-hover:scale-110 pointer-events-none overflow-hidden" />

      <div className="flex items-start gap-4 mb-6 relative">
        <div className="p-3 rounded-xl bg-indigo-50 dark:bg-zinc-800/50 border border-indigo-100 dark:border-white/5 text-indigo-600 dark:text-indigo-400 shadow-sm">
          <Icon className="h-5 w-5" />
        </div>
        <div className="space-y-1">
          <h3 className="text-lg font-bold text-zinc-900 dark:text-white tracking-tight">{title}</h3>
          {description && <p className="text-sm text-zinc-500">{description}</p>}
        </div>
      </div>
      <div className="relative">
        {children}
      </div>
    </motion.div>
  );
});
