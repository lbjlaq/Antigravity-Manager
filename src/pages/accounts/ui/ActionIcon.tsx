// File: src/pages/accounts/ui/ActionIcon.tsx
import { memo } from 'react';
import { cn } from '@/shared/lib';
import { LucideIcon } from 'lucide-react';

interface ActionIconProps {
  icon: LucideIcon;
  onClick: () => void;
  label: string;
  tooltip?: string;
  className?: string;
  iconSize?: number;
}

export const ActionIcon = memo(function ActionIcon({
  icon: Icon,
  onClick,
  label,
  tooltip,
  className,
  iconSize = 16,
}: ActionIconProps) {
  return (
    <button
      onClick={onClick}
      title={tooltip}
      className={cn(
        "h-10 px-3 rounded-xl flex items-center gap-2 transition-all border border-transparent",
        "text-zinc-400 hover:text-white hover:bg-zinc-800 hover:border-white/10",
        className
      )}
    >
      <Icon className="w-4 h-4" style={{ width: iconSize, height: iconSize }} />
      <span className="text-xs font-bold tracking-wide">{label}</span>
    </button>
  );
});
