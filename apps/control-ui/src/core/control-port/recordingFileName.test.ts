import { expect, it } from 'vitest';
import { recordingFileName } from './recordingFileName';

it('names takes with a local timestamp and separates same-millisecond requests', () => {
  const time = new Date(2026, 8, 9, 1, 2, 3, 4);
  const first = recordingFileName(time);
  const second = recordingFileName(time);
  expect(first).toMatch(/^MixRelay-2026-09-09_01-02-03-004_\d+\.wav$/);
  expect(first).not.toBe(second);
  expect(first).not.toMatch(/[\\/:]/);
});
