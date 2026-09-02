import type { ButtonProps } from '@mantine/core';
import { Button } from '@mantine/core';
import type { ComponentPropsWithoutRef } from 'react';

type ActionButtonProps = ButtonProps & Omit<ComponentPropsWithoutRef<'button'>, keyof ButtonProps>;

export function ActionButton(props: ActionButtonProps) {
  return <Button className="action-button" color="signal" {...props} />;
}
