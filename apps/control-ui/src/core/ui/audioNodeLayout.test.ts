import { expect, it } from 'vitest';
import {
  audioNodeFaceGeometry,
  audioNodePortTop,
  channelRowMetrics,
  minimumAudioNodeFaceHeight,
} from './audioNodeLayout';

it.each(Array.from({ length: 64 }, (_, index) => index + 1))(
  'reserves the complete meter labels and bottom padding for %i channels',
  (channels) => {
    const rows = Math.ceil(channels / (channels > 32 ? 16 : 8));
    const content = channels <= 8 ? channels * 10 + (channels - 1) * 3 : rows * 35 + (rows - 1) * 4;
    for (const detail of [false, true]) {
      expect(
        minimumAudioNodeFaceHeight(channels, detail) - 26 - 23 - 14 - 2 - (detail ? 15 : 0),
      ).toBeGreaterThanOrEqual(content);
    }
  },
);

it('keeps the normal stereo face compact and reserves space for the eighth label', () => {
  expect(minimumAudioNodeFaceHeight(0)).toBe(116);
  expect(minimumAudioNodeFaceHeight(2)).toBe(132);
  expect(minimumAudioNodeFaceHeight(8)).toBe(166);
});

it.each(Array.from({ length: 8 }, (_, index) => index + 1))(
  'fits %i recorder rows and anchors every cable at its meter center',
  (channels) => {
    const geometry = audioNodeFaceGeometry({ channels, portCount: channels, mode: 'channel-rows' });
    const { headerHeight, bodyPadding, meterMargin, rowHeight, rowGap, borderWidth } =
      channelRowMetrics;
    expect(geometry.height).toBe(39 + channels * 13);
    for (let index = 0; index < channels; index += 1) {
      const center =
        borderWidth +
        headerHeight +
        bodyPadding +
        meterMargin +
        rowHeight / 2 +
        index * (rowHeight + rowGap);
      expect(audioNodePortTop(geometry, index, channels)).toBe(center);
    }
    // A multi-cable preview must use the same row pitch as the actual handles.
    expect(
      audioNodePortTop(geometry, channels - 1, channels) - audioNodePortTop(geometry, 0, channels),
    ).toBe((channels - 1) * 13);
  },
);

it('preserves standard faces and their distributed port positions', () => {
  const stereo = audioNodeFaceGeometry({ channels: 2, portCount: 2 });
  expect(stereo.height).toBe(132);
  expect([audioNodePortTop(stereo, 0, 2), audioNodePortTop(stereo, 1, 2)]).toEqual([44, 88]);
  expect(audioNodeFaceGeometry({ channels: 64, portCount: 64 }).height).toBe(780);
});

it.each([1, 2, 4, 8])('keeps an unmetered plug-in header compact with %i ports', (ports) => {
  const geometry = audioNodeFaceGeometry({ mode: 'compact', portCount: ports });
  expect(geometry.height).toBe(26);
  expect(geometry.height).toBeLessThan(minimumAudioNodeFaceHeight(ports));
});

it.each([1, 2, 8])(
  'places %i plug-in ports below the header and above the resize grip',
  (count) => {
    const geometry = {
      ...audioNodeFaceGeometry({ mode: 'compact' }),
      height: 300,
      portInsetTop: 26,
      portInsetBottom: 12,
    };
    for (let index = 0; index < count; index++) {
      const top = audioNodePortTop(geometry, index, count);
      expect(top).toBeGreaterThan(26);
      expect(top).toBeLessThan(288);
    }
  },
);
