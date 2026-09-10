import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { ControlPort } from '../../core/control-port/types';
import { HierarchicalMenuButton } from '../../core/ui/HierarchicalMenu';

type Action = 'new' | 'open' | 'save' | 'save_as';
export function WorkspaceFileMenu({
  controlPort,
  disabled,
  run,
}: {
  controlPort: ControlPort;
  disabled: boolean;
  run: (action: Action, locale: string) => Promise<void>;
}) {
  const { i18n } = useTranslation();
  const korean = i18n.language.startsWith('ko');
  const current = useRef({ disabled, run, locale: i18n.language });
  current.current = { disabled, run, locale: i18n.language };
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void controlPort
      .onWorkspaceFile?.((action) => {
        const value = current.current;
        if (!value.disabled) void value.run(action, value.locale);
      })
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else stop = unsubscribe;
      })
      .catch((error: unknown) => console.error('Workspace menu listener failed', error));
    return () => {
      disposed = true;
      stop?.();
    };
  }, [controlPort]);
  if (!controlPort.workspaceFile) return null;
  return (
    <HierarchicalMenuButton
      label={korean ? '파일' : 'File'}
      buttonContent={korean ? '파일' : 'File'}
      items={(
        [
          ['new', korean ? '새 작업 공간' : 'New Workspace'],
          ['open', korean ? '열기…' : 'Open…'],
          ['save', korean ? '저장' : 'Save'],
          ['save_as', korean ? '다른 이름으로 저장…' : 'Save As…'],
        ] as const
      ).map(([id, label]) => ({ id, label, disabled }))}
      onSelect={(id) => {
        if (!disabled && (id === 'new' || id === 'open' || id === 'save' || id === 'save_as'))
          void run(id, i18n.language);
      }}
    />
  );
}
