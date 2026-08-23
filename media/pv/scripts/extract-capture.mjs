import {readFileSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {dirname, resolve} from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const input = resolve(here, '../public/obp1-radar-8-scans.obcap');
const output = resolve(here, '../src/radar-scans.json');

const records = readFileSync(input, 'utf8')
  .trim()
  .split('\n')
  .slice(1)
  .map((line) => JSON.parse(line));

const bytes = (hex) => Buffer.from(hex.replaceAll(' ', ''), 'hex');

const crc16Modbus = (data) => {
  let crc = 0xffff;
  for (const byte of data) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) {
      crc = crc & 1 ? (crc >> 1) ^ 0xa001 : crc >> 1;
    }
  }
  return crc & 0xffff;
};

const exchanges = [];
let active = null;
for (const record of records) {
  if (record.dir === 'tx') {
    if (active) exchanges.push(active);
    active = {tx: bytes(record.hex), rx: [], ts: record.ts_ms};
  } else if (record.dir === 'rx' && active) {
    active.rx.push(bytes(record.hex));
  }
}
if (active) exchanges.push(active);

const scans = exchanges
  .filter(({tx}) => tx.length === 10 && tx.subarray(0, 4).equals(Buffer.from([0x4f, 0x42, 1, 1])))
  .map(({tx, rx, ts}) => {
    const frame = Buffer.concat(rx);
    if (frame.length !== 196) throw new Error(`Expected 196 bytes, got ${frame.length}`);
    const expectedCrc = frame.readUInt16LE(frame.length - 2);
    const actualCrc = crc16Modbus(frame.subarray(0, -2));
    const count = frame[12];
    const points = Array.from({length: count}, (_, index) => {
      const at = 14 + index * 5;
      return {
        angleDeg: frame.readUInt16LE(at) / 100,
        distanceMm: frame.readUInt16LE(at + 2),
        intensity: frame[at + 4],
      };
    });
    return {
      ts,
      seq: frame.readUInt16LE(4),
      uptimeMs: frame.readUInt32LE(8),
      totalLen: frame.readUInt16LE(6),
      simulatedScene: frame[13] & 1,
      crcValid: expectedCrc === actualCrc,
      txHex: tx.toString('hex').match(/.{1,2}/g).join(' ').toUpperCase(),
      rxHex: frame.toString('hex').match(/.{1,2}/g).join(' ').toUpperCase(),
      points,
    };
  });

if (scans.length !== 8 || scans.some((scan) => !scan.crcValid || scan.points.length !== 36)) {
  throw new Error('Capture did not contain eight valid 36-point OBP/1 scans');
}

writeFileSync(output, `${JSON.stringify(scans, null, 2)}\n`);
console.log(`Wrote ${scans.length} verified scans to ${output}`);
