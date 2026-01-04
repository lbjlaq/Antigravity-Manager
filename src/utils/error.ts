/**
 * Type-safe error handling utilities
 */

/**
 * Extracts a meaningful error message from any thrown value
 */
export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  if (error && typeof error === 'object' && 'message' in error) {
    return String(error.message);
  }
  return 'An unknown error occurred';
}

/**
 * Type guard to check if an error has a specific code
 */
export function hasErrorCode(
  error: unknown,
  code: string
): error is { code: string } {
  return (
    error !== null &&
    typeof error === 'object' &&
    'code' in error &&
    error.code === code
  );
}

/**
 * Application-specific error class with typed error codes
 */
export type AppErrorCode =
  | 'NETWORK_ERROR'
  | 'AUTH_ERROR'
  | 'VALIDATION_ERROR'
  | 'NOT_FOUND'
  | 'PERMISSION_DENIED'
  | 'QUOTA_EXCEEDED'
  | 'UNKNOWN';

export class AppError extends Error {
  public readonly code: AppErrorCode;
  public override readonly cause?: unknown;

  constructor(
    message: string,
    code: AppErrorCode = 'UNKNOWN',
    cause?: unknown
  ) {
    super(message);
    this.name = 'AppError';
    this.code = code;
    this.cause = cause;
  }
}

/**
 * Wraps an async operation with proper error handling
 */
export async function withErrorHandling<T>(
  operation: () => Promise<T>,
  errorHandler?: (error: unknown) => void
): Promise<T | null> {
  try {
    return await operation();
  } catch (error) {
    if (errorHandler) {
      errorHandler(error);
    }
    if (import.meta.env.DEV) {
      console.error('[Error]', getErrorMessage(error));
    }
    return null;
  }
}
