import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../../theme/index.css'
import './viewer.css'
import { ViewerApp } from './App'

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('missing #root element in viewer/index.html')
}
createRoot(rootEl).render(
  <StrictMode>
    <ViewerApp />
  </StrictMode>,
)
