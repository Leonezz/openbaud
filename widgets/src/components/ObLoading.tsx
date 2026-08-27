export interface ObLoadingProps {
  /** Shimmer bar widths, one bar per entry (e.g. ["100%", "82%", "58%"]). */
  widths?: readonly string[]
}

export function ObLoading({ widths = ['100%', '82%', '58%'] }: ObLoadingProps) {
  return (
    <div style={{ display: 'grid', gap: 10 }} aria-busy="true">
      {widths.map((width, i) => (
        <div key={i} className="ob-loading" style={{ width }} />
      ))}
    </div>
  )
}
