import assert from 'node:assert/strict';
import test from 'node:test';
import { execFileSync } from 'node:child_process';
import { cloudConfig, cloudNetwork, guestDiskSize, qemuArguments } from './github-vm.mjs';

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

test('disposable guests disable only needrestart kernel-image scanning before apt runs', () => {
  const config = JSON.parse(cloudConfig('ssh-ed25519 AAAA test').split('\n').slice(1).join('\n'));
  const file = config.write_files.find(
    (entry) => entry.path === '/etc/needrestart/conf.d/99-mixrelay-qualification.conf',
  );
  assert.equal(file.permissions, '0644');
  const settings = execFileSync(
    'perl',
    [
      '-e',
      'our %nrconf = (kernelhints => 1, restart => "i", interpscan => 1); eval $ARGV[0]; die $@ if $@; print join("\\t", map { "$_=$nrconf{$_}" } sort keys %nrconf);',
      file.content,
    ],
    { encoding: 'utf8' },
  );
  assert.deepEqual(settings.split('\t'), ['interpscan=1', 'kernelhints=0', 'restart=i']);
});
test('QEMU uses the native guest CPU and binds SSH only to the hosted runner loopback', () => {
  const settings = {
    disk: '/tmp/disk',
    seed: '/tmp/seed',
    firmware: '/tmp/code',
    variables: '/tmp/vars',
    port: 22022,
    serial: '/tmp/serial',
  };
  const arm = qemuArguments({ ...settings, cpuArch: 'arm64', accelerator: 'tcg' });
  assert.equal(arm[1], 'virt,accel=tcg');
  assert.equal(arm[3], 'cortex-a72');
  assert.ok(arm.includes('user,id=net0,hostfwd=tcp:127.0.0.1:22022-:22'));
  const network = cloudNetwork().ethernets.qa;
  assert.equal(network.dhcp4, true);
  assert.equal(network.dhcp6, false);
  assert.ok(
    arm.includes(`virtio-net-pci,netdev=net0,mac=${network.match.macaddress},romfile=`),
    'DHCP must match the actual NIC without requiring a PXE ROM',
  );
  const x64 = qemuArguments({ ...settings, cpuArch: 'x64', accelerator: 'kvm' });
  assert.equal(x64[1], 'q35,accel=kvm');
  assert.equal(x64[3], 'host');
  assert.throws(() => qemuArguments({ ...settings, cpuArch: 'arm64', accelerator: 'fake' }));
});

test('candidate transfer includes the pinned baseline directory in the same guest', async (t) => {
  const { mkdtempSync, mkdirSync, writeFileSync, rmSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { copyCandidates } = await import('./github-vm.mjs');
  const root = mkdtempSync(join(tmpdir(), 'mixrelay-vm-inputs-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'baseline'));
  writeFileSync(join(root, 'candidate.deb'), 'candidate');
  writeFileSync(join(root, 'baseline/baseline.deb'), 'baseline');
  const calls = [];
  await copyCandidates(async (...args) => calls.push(args), ['-i', '/private/key'], 22222, root);
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0][1], [
    '-i',
    '/private/key',
    '-P',
    '22222',
    '-r',
    join(root, 'baseline'),
    join(root, 'candidate.deb'),
    'qa@127.0.0.1:/home/qa/candidate/',
  ]);
  assert.equal(calls[0][2].timeout, 300000);
});
