import { readFileSync } from 'node:fs';
import streamDeck from '@elgato/streamdeck';
import { actionIds, actionSettings } from './action-settings.mjs';
import { exchange } from './client.mjs';
import { ControlSession } from './control-session.mjs';
import { presentation } from './controls.mjs';
import { keyImage } from './key-image.mjs';
import { VolumeGesture } from './volume-gesture.mjs';
import { Feedback } from './feedback.mjs';

const actions = new Map();
const session = new ControlSession(exchange, publish);
const volume = new VolumeGesture(session);
const feedback = new Feedback((entry) => render(entry).catch((error) => streamDeck.logger.warn(String(error))));
const icons = new Map(
  Object.entries(actionIds).map(([name, id]) => [
    id,
    readFileSync(new URL(`../images/${name}.svg`, import.meta.url), 'utf8'),
  ]),
);

async function render(entry) {
  const view = presentation(session.snapshot, entry.settings);
  view.value = streamDeck.i18n.translate(view.value);
  if (entry.action.isDial()) {
    const value = { title: view.title, value: view.value };
    const image = JSON.stringify(value);
    if (image === entry.image) return;
    await entry.action.setFeedback(value);
    entry.image = image;
  } else {
    const image = keyImage(view, entry.settings.operation, icons.get(entry.action.manifestId));
    if (image === entry.image) return;
    await entry.action.setImage(image);
    entry.image = image;
  }
}

async function publish() {
  for (const entry of actions.values()) feedback.update(entry);
  await streamDeck.ui.sendToPropertyInspector({
    type: 'snapshot',
    snapshot: session.snapshot ?? null,
  });
}

async function control(event, operation, ticks) {
  const entry = actions.get(event.action.id);
  if (!entry) return;
  try {
    if (operation !== undefined && entry.action.manifestId !== actionIds.volume)
      throw new Error('This action does not support dial controls');
    if (entry.action.manifestId === actionIds.volume && operation !== 'mute') {
      const delta = ticks ?? (entry.settings.operation === 'gain_down' ? -1 : 1);
      await volume.change(event.action.id, entry.settings, delta);
    } else {
      volume.cancel();
      await session.control(entry.settings, operation, ticks);
    }
  } catch (error) {
    streamDeck.logger.warn(String(error));
    await event.action.showAlert();
  }
}

function update(event) {
  volume.cancel(event.action.id);
  actions.set(event.action.id, {
    ...actions.get(event.action.id),
    action: event.action,
    settings: actionSettings(
      event.action.manifestId,
      event.payload.settings,
      event.action.isDial(),
    ),
  });
  feedback.update(actions.get(event.action.id));
}

streamDeck.actions.onWillAppear(update);
streamDeck.settings.onDidReceiveSettings(update);
streamDeck.actions.onWillDisappear((event) => {
  volume.cancel(event.action.id);
  feedback.remove(event.action.id);
  actions.delete(event.action.id);
});
streamDeck.actions.onKeyDown((event) => {
  void control(event).catch((error) => streamDeck.logger.warn(String(error)));
});
streamDeck.actions.onDialDown((event) => {
  void control(event, 'mute').catch((error) => streamDeck.logger.warn(String(error)));
});
streamDeck.actions.onDialRotate((event) => {
  void control(event, 'gain_up', event.payload.ticks).catch((error) =>
    streamDeck.logger.warn(String(error)),
  );
});
streamDeck.ui.onSendToPlugin((event) => {
  if (event.payload?.type === 'snapshot')
    void publish().catch((error) => streamDeck.logger.warn(String(error)));
});
await streamDeck.connect();
await session.refresh();
setInterval(() => {
  void session.refresh().catch((error) => streamDeck.logger.warn(String(error)));
}, 1000);
