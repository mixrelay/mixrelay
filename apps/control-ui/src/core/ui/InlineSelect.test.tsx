import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { MixRelayProvider } from '../theme/MixRelayProvider';
import { InlineSelect } from './InlineSelect';

const data = ['32', '64', '128', '256', '512', '1024', '2048'].map((value) => ({
  value,
  label: value,
}));

it('keeps the compact controlled value and exposes a portalled, scrollable option list', () => {
  const onChange = vi.fn();
  const view = render(
    <MixRelayProvider>
      <InlineSelect
        label="Buffer size"
        value="128"
        data={data}
        title="Brief audio interruption"
        onChange={onChange}
      />
    </MixRelayProvider>,
  );
  const input = screen.getByRole('combobox', { name: 'Buffer size' });
  expect(input).toHaveValue('128');
  expect(input.closest('[title]')).toHaveAttribute('title', 'Brief audio interruption');
  fireEvent.click(input);
  expect(screen.getByRole('option', { name: '2048' })).toBeInTheDocument();
  expect(view.container.querySelector('[role="option"]')).toBeNull();
  expect(document.querySelector('.mantine-ScrollArea-root')).not.toBeNull();
  fireEvent.click(screen.getByRole('option', { name: '2048' }));
  expect(onChange).toHaveBeenCalledExactlyOnceWith('2048');
  expect(input).toHaveValue('128');
  view.rerender(
    <MixRelayProvider>
      <InlineSelect label="Buffer size" value="2048" data={data} onChange={onChange} />
    </MixRelayProvider>,
  );
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('2048');
});

it('submits the current option once for same-value retry', () => {
  const onChange = vi.fn();
  render(
    <MixRelayProvider>
      <InlineSelect label="Buffer size" value="2048" data={data} onChange={onChange} />
    </MixRelayProvider>,
  );
  const input = screen.getByRole('combobox', { name: 'Buffer size' });
  fireEvent.click(input);
  fireEvent.click(screen.getByRole('option', { name: '2048' }));
  expect(onChange).toHaveBeenCalledExactlyOnceWith('2048');
  expect(input).toHaveValue('2048');
});

it('preserves disabled, Escape, Arrow and Enter selection semantics without clearing', () => {
  const onChange = vi.fn();
  const view = render(
    <MixRelayProvider>
      <InlineSelect label="Buffer size" value="128" data={data} disabled onChange={onChange} />
    </MixRelayProvider>,
  );
  let input = screen.getByRole('combobox', { name: 'Buffer size' });
  fireEvent.click(input);
  expect(input).toBeDisabled();
  expect(screen.queryByRole('option')).not.toBeInTheDocument();
  view.rerender(
    <MixRelayProvider>
      <InlineSelect label="Buffer size" value="128" data={data} onChange={onChange} />
    </MixRelayProvider>,
  );
  input = screen.getByRole('combobox', { name: 'Buffer size' });
  fireEvent.click(input);
  fireEvent.keyDown(input, { key: 'Escape' });
  expect(screen.queryByRole('option')).not.toBeInTheDocument();
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.click(input);
  fireEvent.keyDown(input, { key: 'ArrowDown', code: 'ArrowDown' });
  fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange.mock.calls[0]?.[0]).toBeTypeOf('string');
  expect(onChange.mock.calls[0]?.[0]).not.toBeNull();
  expect(screen.queryByRole('button', { name: /clear/i })).not.toBeInTheDocument();
});
