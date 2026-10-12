import { type KeyboardEvent, type RefObject, useMemo, useState } from 'react';

/** Focuses a node's outer group so a dialog opened next restores focus to it on close. */
export function focusGraphNode(root: HTMLElement, nodeId: string) {
  const owner = [...root.querySelectorAll<HTMLElement>('.react-flow__node')].find(
    (element) => element.dataset.id === nodeId,
  );
  owner?.focus();
}

/** Shift+F10 and the ContextMenu key open the same menu as a secondary click, for the
 * focused node, cable or the canvas itself. Controls inside a node keep their own keys. */
export function openContextMenuFromKeyboard(event: KeyboardEvent<HTMLElement>): boolean {
  const requested =
    event.key === 'ContextMenu' ||
    (event.key === 'F10' && event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey);
  if (!requested || event.repeat || event.nativeEvent.isComposing) return false;
  const target = event.target;
  if (
    !(target instanceof Element) ||
    !(target === event.currentTarget || target.matches('.react-flow__node, .react-flow__edge'))
  )
    return false;
  // The native key would otherwise open the browser menu as well.
  event.preventDefault();
  event.stopPropagation();
  const box = target.getBoundingClientRect();
  target.dispatchEvent(
    new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: box.left + box.width / 2,
      clientY: box.top + box.height / 2,
    }),
  );
  return true;
}

export interface ChannelConnectionRequestState {
  nodeId: string;
  portIds?: readonly string[];
}

/** One owner for the keyboard connection dialog, opened from a node or port selection. */
export function useChannelConnectionRequests(root: RefObject<HTMLElement | null>) {
  const [current, setCurrent] = useState<ChannelConnectionRequestState | null>(null);
  const request = useMemo(
    () => (nodeId: string, portIds?: readonly string[]) => {
      // Requests come from the mounted graph's own menu.
      focusGraphNode(root.current as HTMLElement, nodeId);
      setCurrent({ nodeId, portIds });
    },
    [root],
  );
  return { current, request, close: () => setCurrent(null) };
}
