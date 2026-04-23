// Mock for $app/navigation in Vitest tests.
// Components that call goto() will use this no-op implementation.
import { vi } from 'vitest';

export const goto = vi.fn((_url: string, _opts?: unknown): Promise<void> => Promise.resolve());
export const invalidate = vi.fn();
export const invalidateAll = vi.fn();
export const preloadData = vi.fn();
export const preloadCode = vi.fn();
export const afterNavigate = vi.fn();
export const beforeNavigate = vi.fn();
export const onNavigate = vi.fn();
export const pushState = vi.fn();
export const replaceState = vi.fn();
