// Public synthetic IDs for hermetic build and qualification tests; never release settings.
export const polarEnvironment = {
  MIXRELAY_POLAR_ORGANIZATION_ID: '10000000-0000-4000-8000-000000000001',
  MIXRELAY_POLAR_BENEFIT_ID: '20000000-0000-4000-8000-000000000002',
  MIXRELAY_POLAR_CHECKOUT_URL: 'https://buy.polar.sh/polar_cl_fixture',
};

export function machoSlice(payload, cpu = 0x0100000c) {
  const header = Buffer.alloc(32);
  header.writeUInt32LE(0xfeedfacf);
  header.writeUInt32LE(cpu, 4);
  header.writeUInt32LE(cpu === 0x0100000c ? 0 : 3, 8);
  header.writeUInt32LE(2, 12);
  return Buffer.concat([header, Buffer.from(payload)]);
}

export function fatBinary(payloads, { littleEndian = false, wide = false } = {}) {
  const stride = wide ? 32 : 20;
  const header = Buffer.alloc(8 + payloads.length * stride);
  const write = (value, offset) => header[littleEndian ? 'writeUInt32LE' : 'writeUInt32BE'](value, offset);
  write(wide ? 0xcafebabf : 0xcafebabe, 0);
  write(payloads.length, 4);
  let offset = header.length;
  const slices = payloads.map((payload, index) => {
    const cpu = index === 0 ? 0x0100000c : 0x01000007;
    const slice = machoSlice(payload, cpu);
    const entry = 8 + index * stride;
    write(cpu, entry); write(cpu === 0x0100000c ? 0 : 3, entry + 4);
    if (wide) {
      header[littleEndian ? 'writeBigUInt64LE' : 'writeBigUInt64BE'](BigInt(offset), entry + 8);
      header[littleEndian ? 'writeBigUInt64LE' : 'writeBigUInt64BE'](BigInt(slice.length), entry + 16);
    } else {
      write(offset, entry + 8); write(slice.length, entry + 12);
    }
    offset += slice.length;
    return slice;
  });
  return Buffer.concat([header, ...slices]);
}
