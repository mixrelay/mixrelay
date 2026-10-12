/** Core node frame shared by cards, utility rows and cable anchors. */
export const audioNodeFrame = {
  headerHeight: 26,
  utilityHeight: 24,
  minimumBodyHeight: 68,
  borderWidth: 1,
};

/** Face geometry shared by the card and cable anchors. Options never take part
 * in this calculation. Keep these metrics in sync with AudioNodeCard.module.css. */
export function minimumAudioNodeFaceHeight(
  meterBanks: readonly { channels: number; label?: string }[] = [],
) {
  const banks = meterBanks.filter((bank) => bank.channels > 0);
  const meters = banks.reduce((total, bank) => {
    const count = Math.min(64, Math.max(1, Math.trunc(bank.channels)));
    const rows = Math.ceil(count / (count > 32 ? 16 : 8));
    const content = (count <= 8 ? count * 10 + (count - 1) * 3 : rows * 35 + (rows - 1) * 4) + 2;
    return total + content + (banks.length > 1 && bank.label ? 13 : 0);
  }, 0);
  // Metadata shares the utility row; only meters expand the body.
  const body = Math.max(
    audioNodeFrame.minimumBodyHeight,
    14 + meters + Math.max(0, banks.length - 1) * 3,
  );
  return audioNodeFrame.headerHeight + body + audioNodeFrame.utilityHeight;
}

export const channelRowMetrics = {
  headerHeight: audioNodeFrame.headerHeight,
  bodyPadding: 7,
  meterMargin: 2,
  rowHeight: 10,
  rowGap: 3,
  borderWidth: audioNodeFrame.borderWidth,
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
  meterBanks = [],
  portCount = 0,
  mode = 'standard',
}: {
  meterBanks?: readonly { channels: number; label?: string }[];
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
      height: Math.max(
        minimumAudioNodeFaceHeight(meterBanks),
        audioNodeFrame.headerHeight + audioNodeFrame.utilityHeight + (portCount + 1) * 12,
      ),
      firstChannelCenter: null,
      channelPitch: 0,
      portInsetTop: audioNodeFrame.headerHeight,
      portInsetBottom: audioNodeFrame.utilityHeight,
    };
  const { headerHeight, bodyPadding, meterMargin, rowHeight, rowGap, borderWidth } =
    channelRowMetrics;
  const count = Math.max(1, Math.trunc(meterBanks[0]?.channels ?? 1));
  const rowsHeight = count * rowHeight + (count - 1) * rowGap;
  const bodyHeight = Math.max(
    audioNodeFrame.minimumBodyHeight,
    bodyPadding * 2 + meterMargin + rowsHeight,
  );
  const meterStart =
    headerHeight +
    bodyPadding +
    meterMargin +
    (bodyHeight - bodyPadding * 2 - meterMargin - rowsHeight) / 2;
  return {
    height: headerHeight + bodyHeight,
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
