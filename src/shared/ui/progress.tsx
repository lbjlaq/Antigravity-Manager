// File: src/shared/ui/progress.tsx
import * as React from "react";
import { cn } from "@/shared/lib";

const Progress = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & { value?: number; indicatorClassName?: string }
>(({ className, value, indicatorClassName, ...props }, ref) => (
  <div
    ref={ref}
    className={cn(
      "relative h-4 w-full overflow-hidden rounded-full bg-secondary bg-gray-100 dark:bg-zinc-800",
      className
    )}
    {...props}
  >
    <div
      className={cn("h-full w-full flex-1 bg-primary transition-all bg-gray-900 dark:bg-gray-50", indicatorClassName)}
      style={{ transform: `translateX(-${100 - (value || 0)}%)` }}
    />
  </div>
));
Progress.displayName = "Progress";

export { Progress };
