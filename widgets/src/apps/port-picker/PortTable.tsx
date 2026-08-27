import { Chip } from '../../components/Chip'
import { Led } from '../../components/Led'
import { ObTable, ObTableRow } from '../../components/ObTable'
import { describePort, isBlocked, type EnrichedPort } from './ports'

export interface PortTableProps {
  ports: readonly EnrichedPort[]
  /** canonical path → folded alias paths, so a canonical row can say it has twins. */
  aliases: ReadonlyMap<string, readonly string[]>
  /** Alias rows are listed on their own once the toggle is on. */
  showAliases: boolean
  selectedPath: string | null
  /** Path already handed to the agent — its row keeps the ok LED. */
  sentPath: string | null
  onSelect: (path: string) => void
}

function aliasNote(count: number): string {
  return count === 1 ? '1 alias hidden' : `${count} aliases hidden`
}

export function PortTable({
  ports,
  aliases,
  showAliases,
  selectedPath,
  sentPath,
  onSelect,
}: PortTableProps) {
  return (
    <ObTable
      head={['', 'Port', 'Match']}
      className="pp-table"
      role="radiogroup"
      aria-label="Serial port candidates"
    >
      {ports.map((port) => {
        const selected = port.path === selectedPath
        const blocked = isBlocked(port)
        const folded = showAliases ? [] : (aliases.get(port.path) ?? [])
        const matches = port.matches_devices ?? []
        return (
          <ObTableRow
            key={port.path}
            role="radio"
            selectable={!blocked}
            selected={selected}
            onSelect={() => onSelect(port.path)}
          >
            <td>
              <span className={selected ? 'pp-radio pp-radio--on' : 'pp-radio'} />
            </td>
            <td>
              <div className="pp-path">{port.path}</div>
              <div className="pp-sub">
                {port.path === sentPath && (
                  <>
                    <Led tone="ok" />
                    <span>sent ·</span>
                  </>
                )}
                <span>{describePort(port)}</span>
              </div>
              {blocked && (
                <div className="pp-sub pp-sub--warn">
                  <Led tone="warn" />
                  <span>
                    Held by session {port.open_session} — ask the agent to close that session
                    before picking this port.
                  </span>
                </div>
              )}
              {port.alias_of !== undefined && (
                <div className="pp-sub">
                  <span>same device as {port.alias_of}</span>
                </div>
              )}
            </td>
            <td>
              <div className="pp-match">
                {matches.map((device) => (
                  <Chip key={device} variant="accent">
                    matches {device}
                  </Chip>
                ))}
                {blocked && <Chip variant="warn">in use · {port.open_session}</Chip>}
                {folded.length > 0 && <Chip>{aliasNote(folded.length)}</Chip>}
                {matches.length === 0 && !blocked && folded.length === 0 && (
                  <span className="pp-nomatch">no profile match</span>
                )}
              </div>
            </td>
          </ObTableRow>
        )
      })}
    </ObTable>
  )
}
