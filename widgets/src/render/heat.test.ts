// Unit tests for the pure heatmap helpers (node --test, type stripping).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { gridRange, parseCssColor, rampColor } from './heat.ts'

test('parseCssColor reads 6-digit hex', () => {
  assert.deepEqual(parseCssColor('#17a678'), { r: 0x17, g: 0xa6, b: 0x78 })
})

test('parseCssColor reads 3-digit hex', () => {
  assert.deepEqual(parseCssColor('#fff'), { r: 255, g: 255, b: 255 })
})

test('parseCssColor reads rgba()', () => {
  assert.deepEqual(parseCssColor('rgba(255, 128, 0, 0.5)'), { r: 255, g: 128, b: 0 })
})

test('parseCssColor throws loudly on an unknown form', () => {
  assert.throws(() => parseCssColor('color-mix(in srgb, red, blue)'), /cannot parse color/)
})

test('rampColor interpolates and clamps', () => {
  const lo = { r: 0, g: 0, b: 0 }
  const hi = { r: 100, g: 200, b: 50 }
  assert.equal(rampColor(lo, hi, 0), 'rgb(0, 0, 0)')
  assert.equal(rampColor(lo, hi, 0.5), 'rgb(50, 100, 25)')
  assert.equal(rampColor(lo, hi, 2), 'rgb(100, 200, 50)')
  assert.equal(rampColor(lo, hi, -1), 'rgb(0, 0, 0)')
})

test('gridRange measures min and max', () => {
  assert.deepEqual(gridRange([3, -2, 7, 0]), { min: -2, max: 7 })
})
