import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MixRelayProvider } from '../theme/MixRelayProvider';
import { HierarchicalContextMenu, HierarchicalMenuButton } from './HierarchicalMenu';
import styles from './HierarchicalMenu.module.css';

const viewport = { width: 800, height: 600 };
const menuSize = { width: 200, height: 120 };

function isMenu(element: HTMLElement) {
  return element.getAttribute('role') === 'menu';
}

beforeEach(() => {
  // jsdom has no layout. Supply only measured DOM geometry; Mantine's real
  // cursor anchor and Floating UI middleware still calculate the position.
  vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(viewport.width);
  vi.spyOn(document.documentElement, 'clientHeight', 'get').mockReturnValue(viewport.height);
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return isMenu(this) ? menuSize.width : viewport.width;
  });
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return isMenu(this) ? menuSize.height : viewport.height;
  });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (isMenu(this)) {
      return new DOMRect(
        Number.parseFloat(this.style.left) || 0,
        Number.parseFloat(this.style.top) || 0,
        menuSize.width,
        menuSize.height,
      );
    }
    return new DOMRect(0, 0, viewport.width, viewport.height);
  });
});

afterEach(() => vi.restoreAllMocks());

function openAt(x: number, y: number) {
  render(
    <MixRelayProvider>
      <HierarchicalContextMenu items={[{ id: 'delete', label: 'Delete' }]} onSelect={vi.fn()}>
        <div data-testid="canvas">Canvas</div>
      </HierarchicalContextMenu>
    </MixRelayProvider>,
  );
  fireEvent.contextMenu(screen.getByTestId('canvas'), { clientX: x, clientY: y });
  return screen.findByRole('menu');
}

describe('Context menu cursor placement', () => {
  it('starts at the cursor x coordinate, immediately below the cursor', async () => {
    const menu = await openAt(300, 200);
    await waitFor(() => {
      expect(menu).toHaveAttribute('data-position', 'bottom-start');
      expect(menu).toHaveStyle({ left: '300px', top: '202px' });
    });
  });

  it('flips above the cursor near the bottom edge', async () => {
    const menu = await openAt(300, 590);
    await waitFor(() => {
      expect(menu).toHaveAttribute('data-position', 'top-start');
      expect(menu).toHaveStyle({ left: '300px', top: '468px' });
    });
  });

  it('keeps the entire menu inside the bottom-right viewport edge', async () => {
    const menu = await openAt(798, 598);
    await waitFor(() => {
      const left = Number.parseFloat(menu.style.left);
      const top = Number.parseFloat(menu.style.top);
      expect(menu).toHaveAttribute('data-position', 'top-end');
      expect(left).toBeGreaterThanOrEqual(0);
      expect(left + menuSize.width).toBeLessThanOrEqual(viewport.width);
      expect(top).toBeGreaterThanOrEqual(0);
      expect(top + menuSize.height).toBeLessThan(598);
    });
  });
});

describe.each(['context', 'button'] as const)('%s nested menu', (entry) => {
  it.each(['mouse', 'keyboard'] as const)(
    'keeps all ancestors open until a leaf is selected with %s',
    async (input) => {
      const onSelect = vi.fn();
      const items = [
        {
          id: 'source',
          label: 'Source',
          children: [
            {
              id: 'virtual',
              label: 'Virtual IO',
              children: [{ id: 'create', label: 'New device' }],
            },
            ...Array.from({ length: 30 }, (_, index) => ({
              id: `app-${index}`,
              label: `Application ${index}`,
            })),
          ],
        },
      ];
      render(
        <MixRelayProvider>
          {entry === 'context' ? (
            <HierarchicalContextMenu items={items} onSelect={onSelect}>
              <div data-testid="canvas">Canvas</div>
            </HierarchicalContextMenu>
          ) : (
            <HierarchicalMenuButton
              items={items}
              onSelect={onSelect}
              label="Add"
              buttonContent="Add"
            />
          )}
        </MixRelayProvider>,
      );
      if (entry === 'context')
        fireEvent.contextMenu(screen.getByTestId('canvas'), { clientX: 300, clientY: 200 });
      else fireEvent.click(screen.getByRole('button', { name: 'Add' }));
      for (const name of ['Source', 'Virtual IO']) {
        const item = await screen.findByRole('menuitem', { name });
        if (input === 'mouse') {
          fireEvent.mouseEnter(item);
          await waitFor(() => expect(item).toHaveAttribute('aria-expanded', 'true'));
          fireEvent.mouseDown(item);
          fireEvent.mouseUp(item);
          fireEvent.click(item);
        } else {
          item.focus();
          fireEvent.keyDown(item, { key: 'ArrowRight' });
        }
      }
      const leaf = await screen.findByRole('menuitem', { name: 'New device' });
      const menus = screen.getAllByRole('menu');
      expect(menus).toHaveLength(3);
      // A child must escape every scroll clip without leaving the owning menu
      // (Mantine's outside-click detection depends on DOM containment).
      for (const [index, menu] of menus.entries()) {
        expect(menu.closest(`.${styles.scrollArea}`)).toBeNull();
        if (index > 0) expect(menus[index - 1]).toContainElement(menu);
      }
      expect(onSelect).not.toHaveBeenCalled();
      if (input === 'mouse') {
        fireEvent.mouseDown(leaf);
        fireEvent.mouseUp(leaf);
        fireEvent.click(leaf);
      } else {
        leaf.focus();
        fireEvent.keyDown(leaf, { key: 'Enter' });
        // jsdom does not synthesize a native button click from keyboard activation.
        fireEvent.click(leaf, { detail: 0 });
      }
      expect(onSelect).toHaveBeenCalledExactlyOnceWith('create');
      await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
    },
  );
});

it('shows the reason a disabled menu leaf cannot be selected', async () => {
  const onSelect = vi.fn();
  render(
    <MixRelayProvider>
      <HierarchicalMenuButton
        items={[
          {
            id: 'unavailable',
            label: 'Unavailable input',
            description: 'Device is in use',
            disabled: true,
          },
        ]}
        onSelect={onSelect}
        label="Add"
        buttonContent="Add"
      />
    </MixRelayProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));
  const item = await screen.findByRole('menuitem', { name: /Unavailable input/ });
  expect(item).toBeDisabled();
  expect(item).toHaveAttribute('data-disabled', 'true');
  expect(screen.getByText('Device is in use')).toBeInTheDocument();
  fireEvent.click(item);
  expect(onSelect).not.toHaveBeenCalled();
});
