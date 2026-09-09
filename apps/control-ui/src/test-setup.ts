import '@testing-library/jest-dom/vitest';

import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

afterEach(cleanup);

Object.defineProperty(window, 'matchMedia', {
  configurable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    addEventListener: vi.fn(),
    addListener: vi.fn(),
    dispatchEvent: vi.fn(),
    matches: false,
    media: query,
    onchange: null,
    removeEventListener: vi.fn(),
    removeListener: vi.fn(),
  })),
});

class TestResizeObserver implements ResizeObserver {
  public disconnect() {}
  public observe() {}
  public unobserve() {}
}

globalThis.ResizeObserver = TestResizeObserver;

// jsdom has no layout/scrolling engine. Combobox calls this browser API when
// highlighting an option; model only that platform boundary, not selection.
Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
  configurable: true,
  value: vi.fn(),
});
