import { describe, expect, it } from 'vitest';
import { logoColor, mixrelayVariables, palette, themeRoles } from './tokens';

function luminance(hex: string) {
  const rgb = [1, 3, 5].map((offset) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return (rgb[0] ?? 0) * 0.2126 + (rgb[1] ?? 0) * 0.7152 + (rgb[2] ?? 0) * 0.0722;
}

function contrast(a: string, b: string) {
  const values = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((values[0] ?? 0) + 0.05) / ((values[1] ?? 0) + 0.05);
}

it('keeps all DSUB steps in descending luminance and the brand source fixed', () => {
  for (const steps of Object.values(palette)) {
    expect(Object.keys(steps)).toEqual([
      '50',
      '100',
      '200',
      '300',
      '400',
      '500',
      '600',
      '700',
      '800',
      '900',
    ]);
    const values = Object.values(steps).map(luminance);
    expect(values).toEqual([...values].sort((a, b) => b - a));
  }
  expect(logoColor).toBe('#b02d23');
  expect(mixrelayVariables['--logo-source']).toBe(logoColor);
  expect(Object.keys(themeRoles.light)).toEqual(Object.keys(themeRoles.dark));
  expect(Object.keys(themeRoles.light).some((role) => role.includes('orange'))).toBe(false);
});

describe.each(['light', 'dark'] as const)('%s theme contrast', (scheme) => {
  const roles = themeRoles[scheme];
  const surfaces = [
    '--background',
    '--surface',
    '--surface-muted',
    '--surface-hover',
    '--surface-active',
  ] as const;
  it('keeps normal and semantic text readable on default, hover and pressed surfaces', () => {
    const textRoles = [
      '--text-primary',
      '--text-secondary',
      '--text-muted',
      '--status-accent',
      '--status-positive',
      '--status-warning',
      '--status-danger',
    ] as const;
    for (const surface of surfaces) {
      for (const text of textRoles)
        expect(
          contrast(roles[text], roles[surface]),
          `${text} on ${surface}`,
        ).toBeGreaterThanOrEqual(4.5);
    }
  });
  it('keeps fields, selection and keyboard focus distinguishable', () => {
    for (const surface of surfaces) {
      for (const boundary of [
        '--border-control',
        '--text-primary',
        '--text-secondary',
        '--status-danger',
      ] as const) {
        expect(
          contrast(roles[boundary], roles[surface]),
          `${boundary} on ${surface}`,
        ).toBeGreaterThanOrEqual(3);
      }
    }
  });
  it('keeps strong commands and semantic feedback readable', () => {
    for (const surface of [
      '--button-strong-background',
      '--button-strong-hover',
      '--button-strong-active',
    ] as const)
      expect(contrast(roles['--button-strong-text'], roles[surface])).toBeGreaterThanOrEqual(4.5);
    for (const tone of ['accent', 'positive', 'warning', 'danger'] as const) {
      expect(
        contrast(roles[`--status-${tone}`], roles[`--status-${tone}-surface`]),
      ).toBeGreaterThanOrEqual(4.5);
    }
  });
});
