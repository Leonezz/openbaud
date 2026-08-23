# OpenBaud PV replay demo

This example reproduces the product video's key result without an ESP32 or any
serial hardware. The lossless capture is a real OBP/1 request/response recorded
from the development board on 2026-08-24.

From the repository root:

```sh
cargo run --locked -p openbaud -- run openbaud-pv-board/obp1_radar_scan \
  --workspace examples/pv-demo \
  --port replay:captures/obp1-radar-seq42.obcap \
  --set seq=42
```

Expected evidence:

- `outcome: normal`
- `seq: 42`
- `point_count: 36`
- `simulated_scene: 1`
- 36 decoded `{angle_deg, distance_mm, intensity}` records

`simulated_scene: 1` is important: the ESP32 and serial exchange are real, but
the radar scene is produced by the test firmware. See [protocol.md](protocol.md)
for the exact wire layout.
