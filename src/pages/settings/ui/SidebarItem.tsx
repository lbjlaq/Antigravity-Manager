// File: src/pages/settings/ui/SidebarItem.tsx
import { memo } from 'react';
import { motion } from 'framer-motion';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { LucideIcon } from 'lucide-react';

interface SidebarItemProps {
  active: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}

export const SidebarItem = memo(function SidebarItem({
  active,
  icon: Icon,
  label,
  onClick,
}: SidebarItemProps) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full flex items-center gap-3 px-4 py-3 text-sm font-medium rounded-xl transition-all duration-200 group relative overflow-hidden",
        active
          ? "text-white shadow-lg shadow-indigo-500/20"
          : "text-zinc-500 hover:text-zinc-300 hover:bg-white/5"
      )}
    >
      {active && (
        <motion.div
          layoutId="sidebarActiveItem"
          className="absolute inset-0 bg-gradient-to-r from-indigo-500 to-purple-500"
          transition={{ type: "spring", stiffness: 300, damping: 30 }}
        />
      )}

      {active && (
        <motion.div
          initial={{ x: '-100%' }}
          animate={{ x: '200%' }}
          transition={{ repeat: Infinity, duration: 2, ease: "linear" }}
          className="absolute inset-0 bg-gradient-to-r from-transparent via-white/10 to-transparent skew-x-12"
        />
      )}

      <div className={cn(
        "relative z-10 p-2 rounded-lg transition-colors duration-200",
        active ? "bg-white/20 text-white" : "bg-zinc-800/50 text-zinc-500 group-hover:text-zinc-300 group-hover:bg-zinc-800"
      )}>
        <Icon className="h-4 w-4" />
      </div>

      <span className="relative z-10">{label}</span>

      {active && (
        <motion.div
          initial={{ opacity: 0, x: -10 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.2 }}
          className="absolute right-3 text-white/50 z-10"
        >
          <ChevronRight className="h-4 w-4" />
        </motion.div>
      )}
    </button>
  );
});
