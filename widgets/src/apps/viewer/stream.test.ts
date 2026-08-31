// Unit tests for the live-stream descriptor / stream_poll page validation
// (src/apps/viewer/stream.ts). Runs under `node --test` with Node's built-in
// type stripping — no DOM, no React, pure data in / verdict out.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readStreamDescriptor, readStreamPage } from './stream.ts'

function scopeDescriptor(): Record<string, unknown> {
  return {
    stream: {
      session_id: 's-1',
      parse: { device: 'openbaud-pv-board', command: 'obp1_telemetry' },
    },
    view: { kind: 'scope', y: ['sine', 'saw'] },
  }
}

function heatmapDescriptor(): Record<string, unknown> {
  return {
    stream: {
      session_id: 's-1',
      parse: { device: 'openbaud-pv-board', command: 'obp1_thermal' },
    },
    view: { kind: 'heatmap', data: 'cells', rows: 8, cols: 8 },
  }
}

test('valid scope descriptor', () => {
  const read = readStreamDescriptor(scopeDescriptor(), 'data')
  assert.equal(read.kind, 'stream')
  if (read.kind !== 'stream') return
  assert.equal(read.descriptor.stream.sessionId, 's-1')
  assert.equal(read.descriptor.stream.parse.device, 'openbaud-pv-board')
  assert.equal(read.descriptor.stream.parse.command, 'obp1_telemetry')
  assert.equal(read.descriptor.view.kind, 'scope')
  if (read.descriptor.view.kind !== 'scope') return
  assert.deepEqual(read.descriptor.view.y, ['sine', 'saw'])
})

test('valid heatmap descriptor', () => {
  const read = readStreamDescriptor(heatmapDescriptor(), 'data')
  assert.equal(read.kind, 'stream')
  if (read.kind !== 'stream') return
  assert.equal(read.descriptor.view.kind, 'heatmap')
  if (read.descriptor.view.kind !== 'heatmap') return
  assert.equal(read.descriptor.view.data, 'cells')
  assert.equal(read.descriptor.view.rows, 8)
  assert.equal(read.descriptor.view.cols, 8)
})

function expectInvalid(structured: Record<string, unknown>, needle: string): void {
  const read = readStreamDescriptor(structured, 'data')
  assert.equal(read.kind, 'invalid', `expected invalid, got ${JSON.stringify(read)}`)
  if (read.kind !== 'invalid') return
  assert.ok(
    read.reason.includes(needle),
    `reason ${JSON.stringify(read.reason)} does not name ${JSON.stringify(needle)}`,
  )
}

test('stream must be an object', () => {
  expectInvalid({ ...scopeDescriptor(), stream: 'nope' }, 'data.stream')
})

test('session_id must be a non-empty string', () => {
  const d = scopeDescriptor()
  d.stream = { ...(d.stream as object), session_id: '' }
  expectInvalid(d, 'session_id')
})

test('parse block is required', () => {
  const d = scopeDescriptor()
  d.stream = { session_id: 's-1' }
  expectInvalid(d, 'parse')
})

test('parse.device must be a non-empty string', () => {
  const d = scopeDescriptor()
  d.stream = { session_id: 's-1', parse: { device: 7, command: 'c' } }
  expectInvalid(d, 'parse.device')
})

test('parse.command must be a non-empty string', () => {
  const d = scopeDescriptor()
  d.stream = { session_id: 's-1', parse: { device: 'dev' } }
  expectInvalid(d, 'parse.command')
})

test('view is required', () => {
  const d = scopeDescriptor()
  delete d.view
  expectInvalid(d, 'view')
})

test('a live descriptor only renders scope or heatmap', () => {
  const d = scopeDescriptor()
  d.view = { kind: 'polar', data: 'points', angle: 'a', radius: 'r' }
  expectInvalid(d, '"polar"')
})

test('scope y must be an array of field names', () => {
  const d = scopeDescriptor()
  d.view = { kind: 'scope', y: 'sine' }
  expectInvalid(d, 'view.y')
})

test('scope y rejects an empty list', () => {
  const d = scopeDescriptor()
  d.view = { kind: 'scope', y: [] }
  expectInvalid(d, '1')
})

test('scope y rejects more than 4 series', () => {
  const d = scopeDescriptor()
  d.view = { kind: 'scope', y: ['a', 'b', 'c', 'd', 'e'] }
  expectInvalid(d, '4')
})

test('scope y rejects a non-string entry', () => {
  const d = scopeDescriptor()
  d.view = { kind: 'scope', y: ['a', 3] }
  expectInvalid(d, 'view.y[1]')
})

test('heatmap data must name a field', () => {
  const d = heatmapDescriptor()
  d.view = { kind: 'heatmap', data: '', rows: 8, cols: 8 }
  expectInvalid(d, 'view.data')
})

test('heatmap rows must be a positive integer', () => {
  const d = heatmapDescriptor()
  d.view = { kind: 'heatmap', data: 'cells', rows: 0, cols: 8 }
  expectInvalid(d, 'view.rows')
})

test('heatmap cols rejects a fractional count', () => {
  const d = heatmapDescriptor()
  d.view = { kind: 'heatmap', data: 'cells', rows: 8, cols: 2.5 }
  expectInvalid(d, 'view.cols')
})

// ---- stream_poll page validation ----

function pollPage(): Record<string, unknown> {
  return {
    subscription_id: 'sub-1',
    session_id: 's-1',
    frames: [
      { seq: 0, ts_ms: 100, hex: '01 02', text: '..', parsed: { sine: 0.5 } },
      { seq: 1, ts_ms: 160, hex: '01 02', text: '..', parse_error: 'crc mismatch' },
      { seq: 2, ts_ms: 220, hex: '01 02', text: '..' },
    ],
    next_seq: 3,
    dropped_frames: 0,
    dropped_chunks: 0,
    parse: { device: 'openbaud-pv-board', command: 'obp1_telemetry' },
    units: { sine: 'V', bogus: 42 },
  }
}

test('valid poll page', () => {
  const read = readStreamPage(pollPage())
  assert.equal(read.kind, 'page')
  if (read.kind !== 'page') return
  assert.equal(read.page.subscriptionId, 'sub-1')
  assert.equal(read.page.nextSeq, 3)
  assert.equal(read.page.frames.length, 3)
  assert.deepEqual(read.page.frames[0]?.parsed, { sine: 0.5 })
  assert.equal(read.page.frames[1]?.parseError, 'crc mismatch')
  assert.equal(read.page.frames[2]?.parsed, undefined)
  assert.equal(read.page.frames[2]?.parseError, undefined)
  // non-string unit entries are dropped, string ones survive
  assert.deepEqual(read.page.units, { sine: 'V' })
  assert.equal(read.page.parse?.command, 'obp1_telemetry')
})

function expectBadPage(structured: unknown, needle: string): void {
  const read = readStreamPage(structured)
  assert.equal(read.kind, 'invalid', `expected invalid, got ${JSON.stringify(read)}`)
  if (read.kind !== 'invalid') return
  assert.ok(
    read.reason.includes(needle),
    `reason ${JSON.stringify(read.reason)} does not name ${JSON.stringify(needle)}`,
  )
}

test('page must be an object', () => {
  expectBadPage('nope', 'stream_poll')
})

test('page requires subscription_id', () => {
  const p = pollPage()
  delete p.subscription_id
  expectBadPage(p, 'subscription_id')
})

test('page frames must be an array', () => {
  expectBadPage({ ...pollPage(), frames: 5 }, 'frames')
})

test('frame requires a finite seq', () => {
  const p = pollPage()
  p.frames = [{ ts_ms: 100 }]
  expectBadPage(p, 'frames[0].seq')
})

test('frame requires a finite ts_ms', () => {
  const p = pollPage()
  p.frames = [{ seq: 0, ts_ms: 'soon' }]
  expectBadPage(p, 'frames[0].ts_ms')
})

test('frame parsed must be an object when present', () => {
  const p = pollPage()
  p.frames = [{ seq: 0, ts_ms: 1, parsed: [1, 2] }]
  expectBadPage(p, 'frames[0].parsed')
})

test('frame cannot carry both parsed and parse_error', () => {
  const p = pollPage()
  p.frames = [{ seq: 0, ts_ms: 1, parsed: {}, parse_error: 'x' }]
  expectBadPage(p, 'both')
})

test('page requires dropped_frames', () => {
  const p = pollPage()
  delete p.dropped_frames
  expectBadPage(p, 'dropped_frames')
})

test('page requires next_seq', () => {
  const p = pollPage()
  delete p.next_seq
  expectBadPage(p, 'next_seq')
})
