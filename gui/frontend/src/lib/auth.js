import { createClient } from '@neondatabase/neon-js'

const AUTH_URL = import.meta.env.VITE_NEON_AUTH_URL ?? 'https://neon-auth.example'
const DATA_API_URL = import.meta.env.VITE_NEON_DATA_API_URL ?? 'https://neon-data-api.example'
const REDIRECT_URI =
  import.meta.env.VITE_NEON_REDIRECT_URI ?? 'language-enforcer://auth/callback'

const client = createClient({
  auth: { url: AUTH_URL },
  dataApi: { url: DATA_API_URL }
})

let authState = 'signed_out'

export function getAuthState() {
  return authState
}

export function clearAuth() {
  authState = 'signed_out'
}

export async function startLogin() {
  authState = 'redirecting'
  await client.auth.signIn.social({
    provider: 'google',
    callbackURL: REDIRECT_URI
  })
}

export async function testDataApiRequest() {
  const result = await client.from('words').select('*').order('id', { ascending: false }).limit(1)
  if (result.error) {
    throw new Error(result.error.message)
  }
  authState = 'signed_in'
  return result.data
}
