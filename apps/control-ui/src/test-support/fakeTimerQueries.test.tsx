import { render, screen } from '@testing-library/react';
import { useEffect, useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { advance, eventually, find, findIn } from './fakeTimerQueries';

function Delayed({ ms }: { ms: number }) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), ms);
    return () => clearTimeout(timer);
  }, [ms]);
  return shown ? <button type="button">Ready</button> : null;
}

afterEach(() => vi.useRealTimers());

it('steps the fake clock until a timer-driven element appears, without wall-clock waiting', async () => {
  vi.useFakeTimers();
  render(<Delayed ms={1000} />);
  // No real-clock budget: only stepping the fake clock can reveal the element.
  expect(await find('button', { name: 'Ready' }, { realTimeout: 0 })).toBeInTheDocument();
});

it('fails once the fake-time limit passes', async () => {
  vi.useFakeTimers();
  render(<Delayed ms={5000} />);
  await expect(
    eventually(() => screen.getByRole('button', { name: 'Ready' }), {
      timeout: 500,
      realTimeout: 50,
    }),
  ).rejects.toThrow();
});

/** Shows its button from a MessageChannel message, which fake timers do not control. */
function RealAsync() {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => setShown(true);
    channel.port2.postMessage(null);
    return () => channel.port1.close();
  }, []);
  return shown ? <button type="button">Real</button> : null;
}

it('keeps checking on the real clock once fake time is used up', async () => {
  vi.useFakeTimers();
  render(<RealAsync />);
  expect(await find('button', { name: 'Real' }, { timeout: 0 })).toBeInTheDocument();
});

it('defers to Testing Library under real timers', async () => {
  render(<Delayed ms={10} />);
  await advance();
  expect(await findIn(screen, 'button', { name: 'Ready' })).toBeInTheDocument();
  await eventually(() => expect(screen.getByRole('button', { name: 'Ready' })).toBeVisible());
  await eventually(() => expect(screen.getByRole('button', { name: 'Ready' })).toBeVisible(), {
    timeout: 1000,
  });
});
