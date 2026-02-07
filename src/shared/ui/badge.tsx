// File: src/shared/ui/badge.tsx
import * as React from "react";
import { cn } from "@/shared/lib";

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: "default" | "secondary" | "destructive" | "outline" | "success" | "warning";
}

function Badge({ className, variant = "default", ...props }: BadgeProps) {
  const variants = {
    default: "border-transparent bg-primary text-primary-foreground hover:bg-primary/80 bg-gray-900 text-white dark:bg-gray-50 dark:text-gray-900",
    secondary: "border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80 bg-gray-100 text-gray-900 dark:bg-zinc-800 dark:text-gray-100",
    destructive: "border-transparent bg-destructive text-destructive-foreground hover:bg-destructive/80 bg-red-500 text-white",
    outline: "text-foreground border-gray-200 dark:border-zinc-700 text-gray-700 dark:text-gray-300",
    success: "border-transparent bg-emerald-500 text-white hover:bg-emerald-600",
    warning: "border-transparent bg-amber-500 text-white hover:bg-amber-600",
  };

  return (
    <div
      className={cn(
        "inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 border",
        variants[variant],
        className
      )}
      {...props}
    />
  );
}

export { Badge };
