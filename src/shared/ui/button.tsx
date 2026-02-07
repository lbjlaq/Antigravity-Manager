// File: src/shared/ui/button.tsx
import * as React from "react";
import { cn } from "@/shared/lib";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "destructive" | "outline" | "secondary" | "ghost" | "link";
  size?: "default" | "sm" | "lg" | "icon";
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "default", ...props }, ref) => {
    const variants = {
      default: "bg-primary text-primary-foreground hover:bg-primary/90 bg-gray-900 text-white dark:bg-gray-50 dark:text-gray-900 dark:hover:bg-gray-200",
      destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90 bg-red-500 text-white hover:bg-red-600",
      outline: "border border-input bg-background hover:bg-accent hover:text-accent-foreground border-gray-200 dark:border-zinc-800 hover:bg-gray-100 dark:hover:bg-zinc-800 dark:text-gray-200",
      secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80 bg-gray-100 text-gray-900 dark:bg-zinc-800 dark:text-gray-100",
      ghost: "hover:bg-accent hover:text-accent-foreground hover:bg-gray-100 dark:hover:bg-zinc-800 dark:text-gray-200",
      link: "text-primary underline-offset-4 hover:underline text-blue-600",
    };
    
    const sizes = {
      default: "h-10 px-4 py-2",
      sm: "h-9 rounded-md px-3",
      lg: "h-11 rounded-md px-8",
      icon: "h-10 w-10",
    };

    return (
      <button
        className={cn(
          "inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
          variants[variant],
          sizes[size],
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

export { Button };
