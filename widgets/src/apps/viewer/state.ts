// Viewer state (pure). show_result hands the widget a deliberately tiny
// envelope — { source, path?, uri?, bytes? } — so the model's context never
// carries the payload. The full result JSON is fetched separately: from the
// MCP resource named by `uri` (source "file"), or from the tool input's
// `data` object (source "inline"). Nothing here polls: a saved result has no
// next frame, so there is no previous frame either.
import {
  openbaudStructured,
  type OpenbaudSummary,
  type ReadResourceResult,
  type ToolResult,
} from '../../mcp/useWidget'
import { asNumber, asString, dispatchResult, isRecord, type PolarScan } from './dispatch'

/** Where the full result lives, per the show_result envelope. */
export type ResultRef =
  | {
      readonly source: 'file'
      /** openbaud://result/res-<ms>-<tool>.json — read via MCP resources/read. */
      readonly uri: string
      /** Workspace-relative path the server saved it under, when reported. */
      readonly path: string | undefined
      readonly bytes: number | undefined
    }
  | {
      readonly source: 'inline'
      readonly path: string | undefined
      readonly bytes: number | undefined
    }

export type ViewerState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'loading'; readonly ref: ResultRef }
  | { readonly kind: 'error'; readonly message: string }
  | { readonly kind: 'generic'; readonly structured: OpenbaudSummary; readonly ref: ResultRef }
  | { readonly kind: 'polar'; readonly scan: PolarScan; readonly ref: ResultRef }

export const INITIAL_VIEWER: ViewerState = { kind: 'idle' }

export interface IntensityRange {
  readonly min: number
  readonly max: number
}

/** Range behind both the dot colors and the legend — they must agree. */
export function intensityRange(scan: PolarScan): IntensityRange {
  if (!scan.hasIntensity) {
    // No intensity data: a symmetric fake range parks every 0-intensity point
    // on the middle ramp bin (the legend is hidden in this case).
    return { min: -2, max: 2 }
  }
  let min = Infinity
  let max = -Infinity
  for (const point of scan.points) {
    if (point.intensity < min) min = point.intensity
    if (point.intensity > max) max = point.intensity
  }
  return { min, max }
}

export function errorText(result: ToolResult): string {
  const texts = result.content.flatMap((block) => (block.type === 'text' ? [block.text] : []))
  const joined = texts.join('\n').trim()
  return joined !== '' ? joined : 'tool call failed (host returned no error text)'
}

export type Envelope =
  | { readonly kind: 'ref'; readonly ref: ResultRef }
  | { readonly kind: 'failed'; readonly message: string }

function failed(message: string): Envelope {
  return { kind: 'failed', message }
}

/** Reads the show_result envelope out of a tool result. Never guesses. */
export function readEnvelope(result: ToolResult): Envelope {
  if (result.isError) return failed(errorText(result))
  const structured = openbaudStructured(result)
  if (structured === undefined) {
    return failed('show_result returned no structuredContent — nothing to render')
  }
  const source = structured.source
  const path = asString(structured.path)
  const bytes = asNumber(structured.bytes)
  if (source === 'inline') return { kind: 'ref', ref: { source, path, bytes } }
  if (source !== 'file') {
    return failed(
      `show_result envelope carries source ${JSON.stringify(source)} — expected "file" or "inline"`,
    )
  }
  const uri = asString(structured.uri)
  if (uri === undefined) {
    return failed('show_result reported source "file" but carried no resource uri — nothing to read')
  }
  return { kind: 'ref', ref: { source, uri, path, bytes } }
}

/** contents[0].text of a resources/read result, parsed as the full result JSON. */
export function parseResourceJson(result: ReadResourceResult, uri: string): OpenbaudSummary {
  const first = result.contents[0]
  if (first === undefined) {
    throw new Error(`resource ${uri} came back with no contents`)
  }
  const text = (first as { text?: unknown }).text
  if (typeof text !== 'string') {
    throw new Error(`resource ${uri} carries no text content — a binary resource cannot be rendered`)
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error)
    throw new Error(`resource ${uri} is not valid JSON: ${detail}`)
  }
  if (!isRecord(parsed)) {
    throw new Error(
      `resource ${uri} parsed to ${Array.isArray(parsed) ? 'an array' : typeof parsed} — expected a result object`,
    )
  }
  return parsed as OpenbaudSummary
}

/** Schema dispatch on the full result: polar scope, or the generic KV card. */
export function viewFor(structured: OpenbaudSummary, ref: ResultRef): ViewerState {
  const view = dispatchResult(structured)
  if (view.kind === 'polar') return { kind: 'polar', scan: view.scan, ref }
  return { kind: 'generic', structured, ref }
}
