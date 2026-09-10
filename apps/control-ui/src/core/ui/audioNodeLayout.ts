/** Face geometry shared by the card and cable anchors. Options never take part
 * in this calculation. Keep these metrics in sync with AudioNodeCard.module.css. */
export function minimumAudioNodeFaceHeight(channels = 0, hasDetail = false) {
  if (channels <= 0) return 116;
  const count = Math.min(64, Math.max(1, Math.trunc(channels)));
  const rows = Math.ceil(count / (count > 32 ? 16 : 8));
  const meters = count <= 8 ? count * 10 + (count - 1) * 3 : rows * 35 + (rows - 1) * 4;
  // Header + footer + body padding + meter top margin + optional detail row.
  return Math.max(132, 26 + 23 + 14 + 2 + meters + (hasDetail ? 15 : 0));
}

export const channelRowMetrics = {
  headerHeight: 26,
  bodyPadding: 7,
  meterMargin: 2,
  rowHeight: 10,
  rowGap: 3,
  borderWidth: 1,
};

export type AudioNodeFaceMode = 'standard' | 'channel-rows' | 'compact';

export interface AudioNodeFaceGeometry {
  height: number;
  firstChannelCenter: number | null;
  channelPitch: number;
  portInsetTop?: number;
  portInsetBottom?: number;
}

/** Channel-row faces contain only their header and meter rows. The same row
 * metrics drive CSS and cable centers; disclosure content is outside this face. */
export function audioNodeFaceGeometry({
  channels = 0,
  hasDetail = false,
  portCount = 0,
  mode = 'standard',
}: {
  channels?: number;
  hasDetail?: boolean;
  portCount?: number;
  mode?: AudioNodeFaceMode;
}): AudioNodeFaceGeometry {
  if (mode === 'compact')
    return {
      height: channelRowMetrics.headerHeight,
      firstChannelCenter: null,
      channelPitch: 0,
    };
  if (mode === 'standard')
    return {
      height: Math.max(minimumAudioNodeFaceHeight(channels, hasDetail), (portCount + 1) * 12),
      firstChannelCenter: null,
      channelPitch: 0,
    };
  const { headerHeight, bodyPadding, meterMargin, rowHeight, rowGap, borderWidth } =
    channelRowMetrics;
  const count = Math.max(1, Math.trunc(channels));
  const meterStart = headerHeight + bodyPadding + meterMargin;
  return {
    height: meterStart + count * rowHeight + (count - 1) * rowGap + bodyPadding,
    firstChannelCenter: borderWidth + meterStart + rowHeight / 2,
    channelPitch: rowHeight + rowGap,
  };
}

export function audioNodePortTop(geometry: AudioNodeFaceGeometry, index: number, count: number) {
  const top = geometry.portInsetTop ?? 0;
  const available = geometry.height - top - (geometry.portInsetBottom ?? 0);
  return geometry.firstChannelCenter === null
    ? top + ((index + 1) / (count + 1)) * available
    : geometry.firstChannelCenter + index * geometry.channelPitch;
}
