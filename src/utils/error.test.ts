import { describe, it, expect } from 'vitest';
import {
  getErrorMessage,
  hasErrorCode,
  AppError,
  withErrorHandling,
} from './error';

describe('getErrorMessage', () => {
  it('extracts message from Error instance', () => {
    const error = new Error('Test error');
    expect(getErrorMessage(error)).toBe('Test error');
  });

  it('returns string directly', () => {
    expect(getErrorMessage('String error')).toBe('String error');
  });

  it('extracts message from object with message property', () => {
    const error = { message: 'Object error' };
    expect(getErrorMessage(error)).toBe('Object error');
  });

  it('returns default message for unknown values', () => {
    expect(getErrorMessage(null)).toBe('An unknown error occurred');
    expect(getErrorMessage(undefined)).toBe('An unknown error occurred');
    expect(getErrorMessage(123)).toBe('An unknown error occurred');
    expect(getErrorMessage({})).toBe('An unknown error occurred');
  });
});

describe('hasErrorCode', () => {
  it('returns true for matching error code', () => {
    const error = { code: 'NETWORK_ERROR' };
    expect(hasErrorCode(error, 'NETWORK_ERROR')).toBe(true);
  });

  it('returns false for non-matching error code', () => {
    const error = { code: 'NETWORK_ERROR' };
    expect(hasErrorCode(error, 'AUTH_ERROR')).toBe(false);
  });

  it('returns false for null', () => {
    expect(hasErrorCode(null, 'NETWORK_ERROR')).toBe(false);
  });

  it('returns false for objects without code property', () => {
    expect(hasErrorCode({}, 'NETWORK_ERROR')).toBe(false);
    expect(hasErrorCode({ message: 'error' }, 'NETWORK_ERROR')).toBe(false);
  });
});

describe('AppError', () => {
  it('creates error with message and default code', () => {
    const error = new AppError('Test error');
    expect(error.message).toBe('Test error');
    expect(error.code).toBe('UNKNOWN');
    expect(error.name).toBe('AppError');
    expect(error.cause).toBeUndefined();
  });

  it('creates error with custom code', () => {
    const error = new AppError('Network failed', 'NETWORK_ERROR');
    expect(error.code).toBe('NETWORK_ERROR');
  });

  it('creates error with cause', () => {
    const originalError = new Error('Original');
    const error = new AppError('Wrapped', 'UNKNOWN', originalError);
    expect(error.cause).toBe(originalError);
  });

  it('is instanceof Error', () => {
    const error = new AppError('Test');
    expect(error).toBeInstanceOf(Error);
    expect(error).toBeInstanceOf(AppError);
  });
});

describe('withErrorHandling', () => {
  it('returns result on success', async () => {
    const result = await withErrorHandling(() => Promise.resolve('success'));
    expect(result).toBe('success');
  });

  it('returns null on error', async () => {
    const result = await withErrorHandling(() =>
      Promise.reject(new Error('fail'))
    );
    expect(result).toBeNull();
  });

  it('calls error handler on error', async () => {
    let capturedError: unknown;
    await withErrorHandling(
      () => Promise.reject(new Error('fail')),
      (err) => {
        capturedError = err;
      }
    );
    expect(capturedError).toBeInstanceOf(Error);
    expect((capturedError as Error).message).toBe('fail');
  });

  it('handles async operations correctly', async () => {
    const asyncOperation = async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
      return 42;
    };
    const result = await withErrorHandling(asyncOperation);
    expect(result).toBe(42);
  });
});
