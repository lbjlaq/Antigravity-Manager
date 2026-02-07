// File: src/shared/ui/skeleton.tsx
import { cn } from "@/shared/lib";

function Skeleton({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("animate-pulse rounded-md bg-zinc-100 dark:bg-zinc-800", className)}
      {...props}
    />
  );
}

export { Skeleton };
