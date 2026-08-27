#!/usr/bin/env node
// Zero-dependency DTCG token → CSS generator.
// Output rules mirror the design-system skill's generate-tokens.cjs so the
// result stays byte-identical to the reviewed docs/design/mcp-apps-ui/tokens.css:
//   - primitive.* keeps its "primitive-" prefix; semantic.* / component.* drop
//     the top-level group from the variable name
//   - dark.semantic.* is emitted as a `.dark { … }` block (the runtime maps the
//     host theme onto both `data-theme` and the `dark` class)
//   - a $value holding a single "{path}" reference resolves recursively; any
//     other string (including multi-reference values) passes through verbatim

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const widgetsRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const inputPath = join(widgetsRoot, 'design-tokens', 'tokens.json')
const outputPath = join(widgetsRoot, 'src', 'theme', 'tokens.css')

function resolveReference(value, tokens) {
  if (typeof value !== 'string' || !value.startsWith('{')) {
    return value
  }
  const path = value.slice(1, -1).split('.')
  let result = tokens
  for (const key of path) {
    result = result?.[key]
  }
  if (result?.$value) {
    return resolveReference(result.$value, tokens)
  }
  return result || value
}

function toCssVarName(path) {
  return '--' + path.join('-').replace(/\./g, '-')
}

function flattenTokens(obj, tokens, prefix = [], result = {}) {
  for (const [key, value] of Object.entries(obj)) {
    const currentPath = [...prefix, key]
    if (value && typeof value === 'object') {
      if (value.$value !== undefined) {
        result[toCssVarName(currentPath)] = resolveReference(value.$value, tokens)
      } else {
        flattenTokens(value, tokens, currentPath, result)
      }
    }
  }
  return result
}

function generateCSS(tokens) {
  const primitive = flattenTokens(tokens.primitive ?? {}, tokens, ['primitive'])
  const semantic = flattenTokens(tokens.semantic ?? {}, tokens, [])
  const component = flattenTokens(tokens.component ?? {}, tokens, [])
  const darkSemantic = flattenTokens(tokens.dark?.semantic ?? {}, tokens, [])

  const block = (vars) =>
    Object.entries(vars)
      .map(([k, v]) => `  ${k}: ${v};`)
      .join('\n')

  let css = `/* Design Tokens - Auto-generated */
/* Do not edit directly - modify tokens.json instead */

/* === PRIMITIVES === */
:root {
${block(primitive)}
}

/* === SEMANTIC === */
:root {
${block(semantic)}
}

/* === COMPONENTS === */
:root {
${block(component)}
}
`

  if (Object.keys(darkSemantic).length > 0) {
    css += `
/* === DARK MODE === */
.dark {
${block(darkSemantic)}
}
`
  }
  return css
}

function main() {
  let raw
  try {
    raw = readFileSync(inputPath, 'utf-8')
  } catch (error) {
    console.error(`cannot read ${inputPath}: ${error.message}`)
    process.exit(1)
  }
  const tokens = JSON.parse(raw)
  const css = generateCSS(tokens)
  mkdirSync(dirname(outputPath), { recursive: true })
  writeFileSync(outputPath, css)
  console.error(`Generated: ${outputPath}`)
}

main()
