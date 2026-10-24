import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('找不到 #root 挂载点，无法启动前端应用')
}

// `?selftest=1` 只在 VITE_SELFTEST=1 的构建里生效（e2e 的 CSP 冒烟用例）。
const selfTestRequested =
  import.meta.env.VITE_SELFTEST === '1' &&
  new URLSearchParams(window.location.search).has('selftest')

if (selfTestRequested) {
  void import('./self-test').then(({ renderSelfTest }) => {
    renderSelfTest(rootElement)
  })
} else {
  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}
