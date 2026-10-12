import { act, render, screen } from '@testing-library/react';
import { useRef } from 'react';
import { expect, it } from 'vitest';
import { TitleField } from './TitleField';
import { useRemovedFocusReturn } from './useRemovedFocusReturn';

function Region({
  shown = true,
  disabled = false,
  inert = false,
  owner = 'graph-1',
}: {
  shown?: boolean;
  disabled?: boolean;
  inert?: boolean;
  owner?: string;
}) {
  const target = useRef<HTMLInputElement>(null);
  const focusReturn = useRemovedFocusReturn(target, owner);
  return (
    <>
      <TitleField ref={target} aria-label="Graph name" disabled={disabled} inert={inert} />
      {shown ? (
        <div {...focusReturn}>
          <button type="button">Reload</button>
          <button type="button">Retry</button>
        </div>
      ) : null}
      <button type="button">Other action</button>
    </>
  );
}

it('returns a removed recovery owner to the remaining input without scrolling', () => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  view.rerender(<Region shown={false} />);
  expect(screen.getByRole('textbox', { name: 'Graph name' })).toHaveFocus();
});
it('keeps a connected recovery owner focused', () => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  view.rerender(<Region />);
  expect(screen.getByRole('button', { name: 'Reload' })).toHaveFocus();
});
it('tracks the next recovery owner when focus moves inside the same region', () => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  act(() => screen.getByRole('button', { name: 'Retry' }).focus());
  view.rerender(<Region shown={false} />);
  expect(screen.getByRole('textbox', { name: 'Graph name' })).toHaveFocus();
});
it('does not take focus back from another action after recovery completes', () => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  act(() => screen.getByRole('button', { name: 'Other action' }).focus());
  view.rerender(<Region shown={false} />);
  expect(screen.getByRole('button', { name: 'Other action' })).toHaveFocus();
});
it('does not restore an explicitly blurred recovery owner', () => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  act(() => screen.getByRole('button', { name: 'Reload' }).blur());
  view.rerender(<Region shown={false} />);
  expect(document.body).toHaveFocus();
});
it.each(['disabled', 'inert'] as const)('skips a %s fallback', (state) => {
  const view = render(<Region />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  view.rerender(<Region shown={false} {...{ [state]: true }} />);
  expect(document.body).toHaveFocus();
});
it('does not restore into a retired region when the workspace owner changes', () => {
  const view = render(<Region key="old" />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  view.rerender(<Region key="new" shown={false} />);
  expect(document.body).toHaveFocus();
});
it('clears the old recovery owner when a persistent region selects another graph', () => {
  const view = render(<Region owner="graph-1" />);
  act(() => screen.getByRole('button', { name: 'Reload' }).focus());
  view.rerender(<Region owner="graph-2" shown={false} />);
  expect(document.body).toHaveFocus();
});
