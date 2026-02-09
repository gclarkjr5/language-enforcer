if (!globalThis.crypto) {
  globalThis.crypto = {}
}

if (!globalThis.crypto.getRandomValues) {
  globalThis.crypto.getRandomValues = (buffer) => {
    for (let i = 0; i < buffer.length; i += 1) {
      buffer[i] = Math.floor(Math.random() * 256)
    }
    return buffer
  }
}

if (!globalThis.crypto.randomUUID) {
  globalThis.crypto.randomUUID = () => {
    const bytes = new Uint8Array(16)
    globalThis.crypto.getRandomValues(bytes)
    bytes[6] = (bytes[6] & 0x0f) | 0x40
    bytes[8] = (bytes[8] & 0x3f) | 0x80
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  }
}
