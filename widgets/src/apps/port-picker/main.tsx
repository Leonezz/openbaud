import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../../theme/index.css'
import './port-picker.css'
import { PortPickerApp } from './App'

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('missing #root element in port-picker/index.html')
}
createRoot(rootEl).render(
  <StrictMode>
    <PortPickerApp />
  </StrictMode>,
)
