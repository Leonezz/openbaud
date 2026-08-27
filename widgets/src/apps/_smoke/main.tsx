import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../../theme/index.css'
import { SmokeApp } from './App'

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('missing #root element in _smoke/index.html')
}
createRoot(rootEl).render(
  <StrictMode>
    <SmokeApp />
  </StrictMode>,
)
