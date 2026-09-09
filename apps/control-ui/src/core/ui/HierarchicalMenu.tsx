import { Button, Image, Menu, Tooltip } from '@mantine/core';
import {
  type MouseEventHandler,
  type PropsWithChildren,
  type ReactNode,
  useRef,
  useState,
} from 'react';

import styles from './HierarchicalMenu.module.css';

export interface HierarchicalMenuItem {
  children?: readonly HierarchicalMenuItem[];
  description?: string;
  disabled?: boolean;
  destructive?: boolean;
  id: string;
  iconUrl?: string;
  kind?: 'divider' | 'item' | 'label';
  label: string;
}

interface MenuContentProps {
  items: readonly HierarchicalMenuItem[];
  onSelect: (itemId: string) => void;
}

export function HierarchicalMenuButton({
  buttonClassName,
  buttonContent,
  label,
  items,
  onClick,
  onOpenedChange,
  onSelect,
  opened,
  tooltip,
}: MenuContentProps & {
  buttonClassName?: string;
  buttonContent: ReactNode;
  label: string;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  onOpenedChange?: (opened: boolean) => void;
  opened?: boolean;
  tooltip?: string;
}) {
  const targetRef = useRef<HTMLButtonElement>(null);
  const button = (
    <Button
      aria-label={label}
      className={buttonClassName}
      onClick={onClick}
      ref={targetRef}
      type="button"
      variant="transparent"
    >
      {buttonContent}
    </Button>
  );
  return (
    <>
      <Menu
        classNames={menuClassNames}
        onChange={onOpenedChange}
        opened={opened}
        position="bottom-start"
        shadow="md"
        offset={4}
        withinPortal
      >
        <Menu.Target>{button}</Menu.Target>
        <Menu.Dropdown className={styles.dropdown}>
          <MenuItems items={items} onSelect={onSelect} />
        </Menu.Dropdown>
      </Menu>
      {tooltip ? (
        <Tooltip label={tooltip} openDelay={350} position="right" target={targetRef} />
      ) : null}
    </>
  );
}

export function HierarchicalContextMenu({
  children,
  disabled,
  items,
  onOpenedChange,
  onSelect,
  opened,
}: PropsWithChildren<
  MenuContentProps & {
    disabled?: boolean;
    onOpenedChange?: (opened: boolean) => void;
    opened?: boolean;
  }
>) {
  return (
    <Menu
      classNames={menuClassNames}
      onChange={onOpenedChange}
      opened={opened}
      position="bottom-start"
      offset={2}
      shadow="md"
      withinPortal
      closeOnClickOutside
      closeOnEscape
    >
      <Menu.ContextMenu disabled={disabled}>{children}</Menu.ContextMenu>
      <Menu.Dropdown className={styles.dropdown}>
        <MenuItems items={items} onSelect={onSelect} />
      </Menu.Dropdown>
    </Menu>
  );
}

function MenuItems({ items, onSelect }: MenuContentProps): ReactNode {
  const [submenuHost, setSubmenuHost] = useState<HTMLDivElement | null>(null);
  return (
    <>
      <div className={styles.scrollArea}>
        {items.map((item) => (
          <MenuEntry item={item} key={item.id} onSelect={onSelect} submenuHost={submenuHost} />
        ))}
      </div>
      {/* Portals stay inside the owning menu, but outside its scroll clip. */}
      <div ref={setSubmenuHost} />
    </>
  );
}

function MenuEntry({
  item,
  onSelect,
  submenuHost,
}: {
  item: HierarchicalMenuItem;
  onSelect: MenuContentProps['onSelect'];
  submenuHost: HTMLDivElement | null;
}): ReactNode {
  if (item.kind === 'divider') {
    return <Menu.Divider className={styles.divider} key={item.id} />;
  }
  if (item.kind === 'label') {
    return (
      <Menu.Label className={styles.label} key={item.id}>
        {item.label}
      </Menu.Label>
    );
  }
  return item.children?.length ? (
    <Menu.Sub
      key={item.id}
      offset={3}
      width={232}
      withinPortal
      portalProps={{ target: submenuHost ?? undefined }}
      floatingStrategy="fixed"
      disabled={item.disabled}
    >
      <Menu.Sub.Target>
        <Menu.Sub.Item className={styles.item} disabled={item.disabled}>
          {item.label}
        </Menu.Sub.Item>
      </Menu.Sub.Target>
      <Menu.Sub.Dropdown className={`${styles.dropdown} ${styles.submenu}`}>
        <MenuItems items={item.children} onSelect={onSelect} />
      </Menu.Sub.Dropdown>
    </Menu.Sub>
  ) : (
    <Menu.Item
      className={styles.item}
      data-description={item.description || undefined}
      data-destructive={item.destructive || undefined}
      disabled={item.disabled}
      key={item.id}
      leftSection={
        item.iconUrl ? (
          <Image alt="" className={styles.itemIcon} height={18} src={item.iconUrl} width={18} />
        ) : undefined
      }
      onClick={() => onSelect(item.id)}
    >
      <span className={styles.itemText}>
        <span className={styles.itemTitle}>{item.label}</span>
        {item.description ? (
          <span className={styles.itemDescription}>{item.description}</span>
        ) : null}
      </span>
    </Menu.Item>
  );
}

const menuClassNames = {
  dropdown: styles.dropdown,
  item: styles.item,
  itemLabel: styles.itemLabel,
  itemSection: styles.itemSection,
};
