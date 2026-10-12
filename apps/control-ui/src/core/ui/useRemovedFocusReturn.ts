import { type FocusEvent, type RefObject, useLayoutEffect, useRef } from 'react';

/** A persistent region returns removed controls to its caller's remaining action. */
export function useRemovedFocusReturn(
  target: RefObject<HTMLElement | null>,
  owner: unknown = target,
) {
  const focused = useRef<HTMLElement | null>(null);
  const previousOwner = useRef(owner);
  useLayoutEffect(() => {
    if (previousOwner.current !== owner) {
      previousOwner.current = owner;
      focused.current = null;
      return;
    }
    const previous = focused.current;
    if (!previous || previous.isConnected) return;
    focused.current = null;
    const next = target.current;
    if (
      document.activeElement !== document.body ||
      !next?.isConnected ||
      next.closest('[inert], [hidden]') ||
      next.matches(':disabled, [aria-disabled="true"]')
    )
      return;
    next.focus({ preventScroll: true });
  });
  const onFocusCapture = (event: FocusEvent<HTMLElement>) => {
    focused.current = event.target;
  };
  const onBlurCapture = (event: FocusEvent<HTMLElement>) => {
    if (event.target.isConnected && !event.currentTarget.contains(event.relatedTarget))
      focused.current = null;
  };
  return { onFocusCapture, onBlurCapture };
}
