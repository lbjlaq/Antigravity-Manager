import { Skeleton } from "@/shared/ui";

export function DashboardSkeleton() {
    return (
        <div className="h-full w-full overflow-hidden bg-zinc-50 dark:bg-zinc-950 p-8 space-y-8">
            <div className="max-w-[1400px] mx-auto space-y-10">
                {/* Header Skeleton */}
                <div className="flex justify-between items-end">
                     <div className="space-y-2">
                        <Skeleton className="h-10 w-64 rounded-lg bg-zinc-200 dark:bg-zinc-800" />
                     </div>
                     <div className="flex gap-2">
                        <Skeleton className="h-9 w-32 rounded-md bg-zinc-200 dark:bg-zinc-800" />
                        <Skeleton className="h-9 w-32 rounded-md bg-zinc-200 dark:bg-zinc-800" />
                     </div>
                </div>

                {/* Stats Row Skeleton */}
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                    {[...Array(4)].map((_, i) => (
                        <div key={i} className="h-32 rounded-xl bg-zinc-900/50 border border-white/5 p-6 flex flex-col justify-between relative overflow-hidden">
                             <div className="flex justify-between items-start">
                                <Skeleton className="h-4 w-20 bg-zinc-800/50" />
                                <Skeleton className="h-8 w-8 rounded bg-zinc-800/50" />
                             </div>
                             <div className="space-y-2">
                                <Skeleton className="h-8 w-16 bg-zinc-800/80" />
                                <Skeleton className="h-3 w-24 bg-zinc-800/30" />
                             </div>
                        </div>
                    ))}
                </div>

                {/* Main Content Grid Skeleton */}
                <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
                    {/* Left Column */}
                    <div className="lg:col-span-7 flex flex-col gap-4">
                        <Skeleton className="h-[400px] w-full rounded-2xl bg-zinc-200 dark:bg-zinc-900/50" />
                        <Skeleton className="h-[200px] w-full rounded-2xl bg-zinc-200 dark:bg-zinc-900/50" />
                    </div>
                    {/* Right Column */}
                    <div className="lg:col-span-5 flex flex-col gap-4">
                        <Skeleton className="h-[250px] w-full rounded-2xl bg-zinc-200 dark:bg-zinc-900/50" />
                        <Skeleton className="h-[350px] w-full rounded-2xl bg-zinc-200 dark:bg-zinc-900/50" />
                    </div>
                </div>
            </div>
        </div>
    );
}
