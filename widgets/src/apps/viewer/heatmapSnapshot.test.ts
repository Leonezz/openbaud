// Unit tests for reading a saved result that declares view: { kind: heatmap }
// (src/apps/viewer/heatmapSnapshot.ts). Runs under `node --test` with Node's
// built-in type stripping — no DOM, no React, pure data in / verdict out.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readHeatmapSnapshot } from './heatmapSnapshot.ts'

const VIEW = { kind: 'heatmap', data: 'cells', rows: 2, cols: 2 }

function thermResult(cells: unknown): Record<string, unknown> {
  return {
    device: 'serial-protocol-tester',
    command: 'therm_snapshot',
    port: '/dev/cu.usbmodem213101',
    outcome: 'normal',
    parsed: { rows: 2, cols: 2, cells },
    units: { cells: 'C' },
    view: VIEW,
  }
}

test('a saved heatmap result reads as a grid with its unit', () => {
  const read = readHeatmapSnapshot(thermResult([24, 23.5, 25, 35.5]), VIEW, 'view')
  assert.equal(read.kind, 'heatmap')
  if (read.kind !== 'heatmap') return
  assert.deepEqual(read.heatmap.values, [24, 23.5, 25, 35.5])
  assert.equal(read.heatmap.rows, 2)
  assert.equal(read.heatmap.cols, 2)
  assert.equal(read.heatmap.data, 'cells')
  assert.equal(read.heatmap.unit, 'C')
  assert.equal(read.heatmap.source, 'serial-protocol-tester · therm_snapshot')
})

test('a grid that does not fit rows x cols names the mismatch', () => {
  const read = readHeatmapSnapshot(thermResult([1, 2, 3]), VIEW, 'view')
  assert.equal(read.kind, 'invalid')
  if (read.kind !== 'invalid') return
  assert.match(read.reason, /3 values/)
  assert.match(read.reason, /2×2 = 4/)
})

test('a non-array or non-numeric cell is a named reason, never a silent blank', () => {
  const notArray = readHeatmapSnapshot(thermResult('24,23'), VIEW, 'view')
  assert.equal(notArray.kind, 'invalid')
  if (notArray.kind === 'invalid') assert.match(notArray.reason, /not an array/)

  const text = readHeatmapSnapshot(thermResult([1, 2, 'x', 4]), VIEW, 'view')
  assert.equal(text.kind, 'invalid')
  if (text.kind === 'invalid') assert.match(text.reason, /non-numeric/)
})

test('a result with no parsed object cannot be drawn', () => {
  const result = { ...thermResult([1, 2, 3, 4]), parsed: undefined }
  const read = readHeatmapSnapshot(result, VIEW, 'view')
  assert.equal(read.kind, 'invalid')
  if (read.kind === 'invalid') assert.match(read.reason, /parsed/)
})

test('the declaration itself is validated: data, rows, cols', () => {
  const view = { kind: 'heatmap', data: 'cells', rows: 0, cols: 2 }
  const read = readHeatmapSnapshot(thermResult([1, 2, 3, 4]), view, 'view')
  assert.equal(read.kind, 'invalid')
  if (read.kind === 'invalid') assert.match(read.reason, /rows must be a positive integer/)
})

test('an encoding override reads the same way as a declared view', () => {
  const result = { ...thermResult([1, 2, 3, 4, 5, 6]), view: undefined }
  const read = readHeatmapSnapshot(result, { kind: 'heatmap', data: 'cells', rows: 2, cols: 3 }, 'encoding')
  assert.equal(read.kind, 'heatmap')
  if (read.kind === 'heatmap') assert.equal(read.heatmap.cols, 3)
})
