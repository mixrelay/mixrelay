import { fireEvent, render, screen, within } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { describe, expect, it, vi } from 'vitest';
import { i18n } from '../../core/i18n';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import { LevelControl } from '../../core/ui/LevelControl';
import { Meter } from '../../core/ui/Meter';
import { ToggleField } from '../../core/ui/ToggleField';
import { broadcastGraph, findCapability, findGraphNode, nodeCatalog } from './model';
import { NodeInventory, StudioGraph } from './StudioGraph';

function renderWithProviders(element: React.ReactNode) {
  return render(
    <ToneNariProvider>
      <I18nextProvider i18n={i18n}>{element}</I18nextProvider>
    </ToneNariProvider>,
  );
}

describe('StudioGraph', () => {
  it('shows a judgeable broadcast graph and updates selection, bypass, and level controls', async () => {
    await i18n.changeLanguage('en');
    const view = renderWithProviders(<StudioGraph />);

    expect(screen.getByRole('region', { name: 'Broadcast mix signal graph' })).toBeInTheDocument();
    expect(screen.getByText('7 nodes')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Compressor' })).toBeInTheDocument();

    const bypass = screen.getByRole('switch', { name: 'Bypass' });
    fireEvent.click(bypass);
    expect(bypass).toBeChecked();
    expect(view.container.querySelector('[data-bypassed="true"]')).toBeInTheDocument();
    fireEvent.click(bypass);
    expect(bypass).not.toBeChecked();

    fireEvent.click(screen.getByRole('button', { name: 'Parametric EQ, 4 bands · Voice clarity' }));
    expect(screen.getByRole('heading', { name: 'Parametric EQ' })).toBeInTheDocument();

    const [outputSlider, mixSlider] = screen.getAllByRole('slider');
    if (!outputSlider || !mixSlider) throw new Error('Expected both inspector sliders');
    fireEvent.keyDown(outputSlider, { key: 'ArrowRight' });
    expect(screen.getByText('-2 dB')).toBeInTheDocument();
    fireEvent.keyDown(mixSlider, { key: 'ArrowLeft' });

    expect(screen.getByTitle('L: 78%')).toBeInTheDocument();
    expect(view.container.querySelectorAll('.palette-item')).toHaveLength(6);
  });

  it('renders the complete capability inventory with explicit implementation states', async () => {
    await i18n.changeLanguage('en');
    renderWithProviders(<NodeInventory />);

    expect(screen.getByRole('heading', { name: 'Node inventory' })).toBeInTheDocument();
    expect(screen.getAllByRole('article')).toHaveLength(nodeCatalog.length);
    expect(screen.getByText('RTMP live')).toBeInTheDocument();
    expect(screen.getByText('Icecast / SHOUTcast')).toBeInTheDocument();
    expect(screen.getAllByText('Foundation').length).toBeGreaterThan(1);
    expect(screen.getAllByText('Planned').length).toBeGreaterThan(1);
    expect(screen.getAllByText('Provider').length).toBeGreaterThan(1);
  });

  it('enforces lookup invariants instead of silently accepting unknown node types', () => {
    expect(findCapability('application').id).toBe('application');
    expect(findGraphNode('chrome').id).toBe('chrome');
    expect(() => findCapability('missing')).toThrow('Unknown node capability: missing');
    expect(() => findGraphNode('missing')).toThrow('Unknown graph node: missing');
    expect(broadcastGraph).toHaveLength(7);
  });
});

describe('Core graph controls', () => {
  it('provides reusable defaults and forwards interactions without Feature importing Mantine', async () => {
    await i18n.changeLanguage('en');
    const onSelect = vi.fn();
    const onToggle = vi.fn();
    const onLevel = vi.fn();
    renderWithProviders(
      <div>
        <AudioNodeCard
          category="Source"
          detail="Test source"
          icon="T"
          inputCount={1}
          onSelect={onSelect}
          outputCount={2}
          title="Reusable node"
        />
        <ToggleField checked={false} label="Enabled" onChange={onToggle} />
        <LevelControl label="Level" max={10} min={0} onChange={onLevel} suffix="%" value={5} />
        <Meter label="Test" level={42} />
      </div>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Reusable node, Test source' }));
    expect(onSelect).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('switch', { name: 'Enabled' }));
    expect(onToggle).toHaveBeenCalledWith(true);
    fireEvent.keyDown(screen.getByRole('slider'), { key: 'ArrowRight' });
    expect(onLevel).toHaveBeenCalledWith(6);
    expect(screen.getByTitle('Test: 42%')).toBeInTheDocument();
    expect(
      within(screen.getByRole('button', { name: 'Reusable node, Test source' })).getByText(
        'Source',
      ),
    ).toBeInTheDocument();
  });
});
