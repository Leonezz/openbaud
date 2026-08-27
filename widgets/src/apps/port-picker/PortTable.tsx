import { Chip } from '../../components/Chip'
import { Led } from '../../components/Led'
import { ObTable, ObTableRow } from '../../components/ObTable'
import { describePort, deviceLabel, type PortCandidate } from './ports'

export interface PortTableProps {
  ports: readonly PortCandidate[]
  selectedPath: string | null
  /** Path of the successfully opened port — its row gains the ok LED. */
  openedPath: string | null
  /** Re-selection is disabled while an open call is in flight or succeeded. */
  selectable: boolean
  onSelect: (path: string) => void
}

export function PortTable({ ports, selectedPath, openedPath, selectable, onSelect }: PortTableProps) {
  return (
    <ObTable
      head={['', 'Port', 'Device']}
      className="pp-table"
      role="radiogroup"
      aria-label="Serial port candidates"
    >
      {ports.map((port) => {
        const selected = port.path === selectedPath
        const opened = port.path === openedPath
        return (
          <ObTableRow
            key={port.path}
            role="radio"
            selectable={selectable}
            selected={selected}
            onSelect={() => onSelect(port.path)}
          >
            <td>
              <span className={selected ? 'pp-radio pp-radio--on' : 'pp-radio'} />
            </td>
            <td>
              <div className="pp-path">{port.path}</div>
              <div className="pp-sub">
                {opened && (
                  <>
                    <Led tone="ok" />
                    <span>open ·</span>
                  </>
                )}
                <span>{describePort(port)}</span>
              </div>
            </td>
            <td>
              <Chip>{deviceLabel(port)}</Chip>
            </td>
          </ObTableRow>
        )
      })}
    </ObTable>
  )
}
