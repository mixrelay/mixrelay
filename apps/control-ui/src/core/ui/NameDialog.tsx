import { Modal, TextInput } from '@mantine/core';
import { useState } from 'react';
import { ActionButton } from './ActionButton';

export function NameDialog({
  title,
  initialName,
  nameLabel,
  createLabel,
  enabled,
  onCancel,
  onCreate,
  error,
}: {
  title: string;
  initialName: string;
  nameLabel: string;
  createLabel: string;
  enabled: boolean;
  onCancel: () => void;
  onCreate: (name: string) => void;
  error?: string | null;
}) {
  const [name, setName] = useState(initialName);
  return (
    <Modal opened onClose={onCancel} title={title} size="xs" centered>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (enabled && name.trim()) onCreate(name.trim());
        }}
      >
        <TextInput
          data-autofocus
          label={nameLabel}
          value={name}
          maxLength={40}
          disabled={!enabled}
          onChange={(event) => setName(event.currentTarget.value)}
        />
        <ActionButton type="submit" mt="md" disabled={!enabled || !name.trim()}>
          {createLabel}
        </ActionButton>
        {error ? <p role="alert">{error}</p> : null}
      </form>
    </Modal>
  );
}
