import assert from 'node:assert/strict';
import test from 'node:test';
import { cloudConfig, qemuArguments, guestDiskSize } from './github-vm.mjs';

test('cloud guest has a locked account and a job-specific SSH key', () => {
  const config = JSON.parse(cloudConfig('ssh-ed25519 AAAA test').split('\n').slice(1).join('\n'));
  assert.equal(config.ssh_pwauth, false);
  assert.equal(config.disable_root, true);
  assert.equal(config.users[0].lock_passwd, true);
  assert.deepEqual(config.users[0].ssh_authorized_keys, ['ssh-ed25519 AAAA test']);
  assert.throws(() => cloudConfig('ssh-ed25519 AAAA\nusers: bad'));
});

test('cloud overlays preserve large disks with trailing boot partitions and grow small disks', () => {
  assert.equal(guestDiskSize({ 'virtual-size': 3 * 1024 ** 3 }), 20 * 1024 ** 3);
  assert.equal(guestDiskSize({ 'virtual-size': 32 * 1024 ** 3 }), 32 * 1024 ** 3);
  assert.throws(() => guestDiskSize({ 'virtual-size': -1 }));
});
test('QEMU uses the native guest CPU and binds SSH only to the hosted runner loopback', () => {
  const settings = { disk: '/tmp/disk', seed: '/tmp/seed', firmware: '/tmp/code', variables: '/tmp/vars', port: 22022, serial: '/tmp/serial' };
  const arm = qemuArguments({ ...settings, cpuArch: 'arm64', accelerator: 'tcg' });
  assert.equal(arm[1], 'virt,accel=tcg');
  assert.equal(arm[3], 'cortex-a72');
  assert.ok(arm.includes('user,id=net0,hostfwd=tcp:127.0.0.1:22022-:22'));
  assert.ok(arm.includes('virtio-net-pci,netdev=net0,romfile='), 'Disk boot must not require an ARM PXE ROM');
  const x64 = qemuArguments({ ...settings, cpuArch: 'x64', accelerator: 'kvm' });
  assert.equal(x64[1], 'q35,accel=kvm');
  assert.equal(x64[3], 'host');
  assert.throws(() => qemuArguments({ ...settings, cpuArch: 'arm64', accelerator: 'fake' }));
});
