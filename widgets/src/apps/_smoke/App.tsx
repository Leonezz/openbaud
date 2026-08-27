import { BadgeSim } from '../../components/Badges'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Led, type LedTone } from '../../components/Led'
import { ObError } from '../../components/ObError'
import { useWidget } from '../../mcp/useWidget'

// Minimal placeholder app: proves the whole chain (tokens → components →
// SDK bridge → single-file build) before the real apps land.
export function SmokeApp() {
  const widget = useWidget({ name: 'openbaud-smoke', version: '0.0.0' })

  const tone: LedTone = widget.connectError ? 'err' : widget.isConnected ? 'ok' : 'off'
  const label = widget.connectError ? 'NO HOST' : widget.isConnected ? 'LIVE' : 'CONNECTING'

  return (
    <div style={{ padding: 16 }}>
      <Card
        title="openbaud widgets"
        head={
          <>
            <BadgeSim>SCAFFOLD</BadgeSim>
            <CardSpacer />
            <Led tone={tone} pulse={tone === 'ok'} />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, fontWeight: 700, letterSpacing: 1 }}>
              {label}
            </span>
          </>
        }
        foot={
          <>
            <span>build pipeline smoke test</span>
            <CardSpacer />
            <Chip>theme: {widget.theme}</Chip>
            <Chip>mode: {widget.displayMode ?? 'n/a'}</Chip>
          </>
        }
      >
        <p style={{ margin: 0 }}>scaffold ok</p>
        {widget.connectError && (
          <div style={{ marginTop: 10 }}>
            <ObError
              title="Host connection failed"
              detail={`${widget.connectError.message} — run inside the harness (pnpm harness) or an MCP Apps host.`}
            />
          </div>
        )}
      </Card>
    </div>
  )
}
