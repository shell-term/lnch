import { useState, useEffect } from 'react'

export default function App() {
  const [status, setStatus] = useState('checking...')

  useEffect(() => {
    fetch('/health')
      .then((r) => r.json())
      .then((data) => setStatus(data.status))
      .catch(() => setStatus('unreachable'))
  }, [])

  return (
    <div style={{ fontFamily: 'sans-serif', padding: '2rem' }}>
      <h1>My App</h1>
      <p>
        API: <strong>{status}</strong>
      </p>
    </div>
  )
}
