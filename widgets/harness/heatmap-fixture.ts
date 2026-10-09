// Saved-result heatmap fixture: a command result declaring
// view: { kind: heatmap } over an 8×8 thermal grid, the shape the
// serial-protocol-tester's GridEYE THERM line decodes to through a numeric
// `split` capture with `unit: C`. The cells are SYNTHETIC and deterministic —
// a ~24 °C ambient field with one warm spot peaking near 35.5 °C, the range
// the real board reported — so the snapshot renders the same on every load.
import type { JsonObject } from './fixtures'

const ROWS = 8
const COLS = 8

function thermalCells(): number[] {
  const cells: number[] = []
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLS; col += 1) {
      const d2 = (row - 5) ** 2 + (col - 2) ** 2
      const warm = 11.5 * Math.exp(-d2 / 2.2)
      const ripple = ((row * 7 + col * 3) % 5) * 0.25
      cells.push(Math.round((23.5 + ripple + warm) * 4) / 4)
    }
  }
  return cells
}

export function heatmapSnapshotResult(): JsonObject {
  const cells = thermalCells()
  return {
    device: 'serial-protocol-tester',
    command: 'therm_snapshot',
    port: '/dev/cu.usbmodem213101',
    tx_hex: '54 48 45 52 4D 3F 0D 0A',
    outcome: 'normal',
    expect: 'normal',
    expect_met: true,
    parsed: {
      rows: ROWS,
      cols: COLS,
      tmax: Math.max(...cells),
      cells,
    },
    units: { tmax: 'C', cells: 'C' },
    view: { kind: 'heatmap', data: 'cells', rows: ROWS, cols: COLS },
  }
}
