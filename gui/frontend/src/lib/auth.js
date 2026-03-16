import { createClient } from '@neondatabase/neon-js'
import { open } from '@tauri-apps/plugin-shell'

const AUTH_URL = import.meta.env.VITE_NEON_AUTH_URL ?? 'https://neon-auth.example'
const DATA_API_URL = import.meta.env.VITE_NEON_DATA_API_URL ?? 'https://neon-data-api.example'
const AUTH_SERVER_URL = import.meta.env.VITE_AUTH_SERVER_URL ?? 'http://127.0.0.1:8787'
const REDIRECT_URI =
  import.meta.env.VITE_NEON_REDIRECT_URI ?? 'language-enforcer://auth.callback'
const IOS_REDIRECT_URI = import.meta.env.VITE_NEON_REDIRECT_URI ?? 'language-enforcer://auth.callback'
const isTauri =
  typeof window !== 'undefined' &&
  (Boolean(window.__TAURI__) || Boolean(window.__TAURI_INTERNALS__))
const isIos =
  typeof navigator !== 'undefined' && /iPad|iPhone|iPod/i.test(navigator.userAgent)

const client = createClient({
  auth: { url: AUTH_URL },
  dataApi: { url: DATA_API_URL }
})

let authState = 'signed_out'
let authToken = null

export function getAuthState() {
  return authState
}

export function getAuthToken() {
  return authToken
}

function extractSession(sessionResult) {
  if (!sessionResult) return null
  if (typeof sessionResult === 'object' && 'data' in sessionResult) {
    return sessionResult.data ?? null
  }
  return sessionResult
}

function extractAccessToken(session) {
  if (!session) return null
  if (session.access_token) return session.access_token
  if (session.session?.access_token) return session.session.access_token
  if (session.data?.session?.access_token) return session.data.session.access_token
  if (session.session?.access_token) return session.session.access_token
  return null
}

async function exchangeSessionToken(token) {
  if (!token) return null
  const response = await fetch(`${AUTH_URL}/token`, {
    headers: {
      accept: 'application/json',
      authorization: `Bearer ${token}`
    }
  })
  if (typeof window !== 'undefined') {
    window.__leAuthTokenExchange = {
      status: response.status,
      ok: response.ok
    }
  }
  if (!response.ok) {
    if (typeof window !== 'undefined') {
      window.__leAuthTokenExchange = {
        status: response.status,
        ok: response.ok,
        body: await response.text()
      }
    }
    return null
  }
  const data = await response.json()
  if (typeof window !== 'undefined') {
    window.__leAuthTokenExchange = {
      status: response.status,
      ok: response.ok,
      body: data
    }
  }
  return data?.token ?? null
}

export function clearAuth() {
  authState = 'signed_out'
  authToken = null
}

export async function refreshAuthState() {
  if (authToken) {
    authState = 'signed_in'
    return null
  }
  const sessionResult = await client.auth.getSession()
  const session = extractSession(sessionResult)
  const token = extractAccessToken(session)
  if (token) authToken = token
  authState = session || authToken ? 'signed_in' : 'signed_out'
  return session
}

export function subscribeToAuthChanges(onChange) {
  if (typeof client.auth.onAuthStateChange !== 'function') {
    return () => {}
  }
  return client.auth.onAuthStateChange((_event, session) => {
    authState = session ? 'signed_in' : 'signed_out'
    if (onChange) onChange(session)
  })
}

export async function requireSession() {
  const session = await refreshAuthState()
  if (!session && !authToken) {
    throw new Error('You must be signed in to use the Data API.')
  }
  return session
}

async function fetchAllWithToken(table, columns) {
  const pageSize = 1000
  let offset = 0
  const rows = []
  for (;;) {
    const url = new URL(`${DATA_API_URL}/${table}`)
    url.searchParams.set('select', columns)
    url.searchParams.set('order', 'id.asc')
    url.searchParams.set('limit', String(pageSize))
    url.searchParams.set('offset', String(offset))
    const response = await fetch(url, {
      headers: {
        accept: 'application/json',
        authorization: `Bearer ${authToken}`
      }
    })
    if (!response.ok) {
      throw new Error(`Data API error: ${response.status} ${await response.text()}`)
    }
    const batch = await response.json()
    rows.push(...batch)
    if (batch.length < pageSize) {
      break
    }
    offset += pageSize
  }
  return rows
}

async function fetchAll(table, columns) {
  const pageSize = 1000
  let from = 0
  const rows = []
  if (authToken) {
    return fetchAllWithToken(table, columns)
  }
  for (;;) {
    const result = await client
      .from(table)
      .select(columns)
      .order('id', { ascending: true })
      .range(from, from + pageSize - 1)
    if (result.error) {
      throw new Error(result.error.message)
    }
    const batch = result.data ?? []
    rows.push(...batch)
    if (batch.length < pageSize) {
      break
    }
    from += pageSize
  }
  return rows
}

export async function fetchDataApiSnapshot() {
  await requireSession()
  const words = await fetchAll(
    'words',
    'id,text,language,translation,chapter,group_name,notes,created_at'
  )
  const cards = await fetchAll('cards', 'id,word_id,due_at,interval_days,ease,reps,lapses')
  const reviews = await fetchAll('reviews', 'id,card_id,grade,reviewed_at')
  const concepts = await fetchAll('concepts', 'id,name,created_at')
  return { words, cards, reviews, concepts }
}

export async function updateWord({ wordId, text, translation }) {
  await requireSession()
  const updates = {}
  if (text !== null && text !== undefined) {
    updates.text = text
  }
  if (translation !== null && translation !== undefined) {
    updates.translation = translation
  }
  if (Object.keys(updates).length === 0) {
    return
  }
  if (authToken) {
    const url = new URL(`${DATA_API_URL}/words`)
    url.searchParams.set('id', `eq.${wordId}`)
    const response = await fetch(url, {
      method: 'PATCH',
      headers: {
        accept: 'application/json',
        authorization: `Bearer ${authToken}`,
        'content-type': 'application/json',
        prefer: 'return=representation'
      },
      body: JSON.stringify(updates)
    })
    if (!response.ok) {
      throw new Error(`Data API error: ${response.status} ${await response.text()}`)
    }
  } else {
    const result = await client.from('words').update(updates).eq('id', wordId).select('id')
    if (result.error) {
      throw new Error(result.error.message)
    }
  }
}

async function deleteRows(url) {
  const response = await fetch(url, {
    method: 'DELETE',
    headers: {
      accept: 'application/json',
      authorization: `Bearer ${authToken}`,
      'content-type': 'application/json'
    }
  })
  if (!response.ok) {
    throw new Error(`Data API error: ${response.status} ${await response.text()}`)
  }
}

export async function deleteWord({ wordId, cardId }) {
  await requireSession()
  if (!authToken) {
    throw new Error('You must be signed in to use the Data API.')
  }
  await deleteRows(`${DATA_API_URL}/reviews?card_id=eq.${cardId}`)
  await deleteRows(`${DATA_API_URL}/cards?id=eq.${cardId}`)
  await deleteRows(`${DATA_API_URL}/words?id=eq.${wordId}`)
}

async function checkWordDuplicate(text, language = 'Dutch') {
  const normalized = text.trim()
  const url = new URL(`${DATA_API_URL}/words`)
  url.searchParams.set('select', 'id,translation')
  url.searchParams.set('language', `eq.${language}`)
  url.searchParams.set('text', `ilike.${normalized}`)
  const response = await fetch(url, {
    headers: {
      accept: 'application/json',
      authorization: `Bearer ${authToken ?? ''}`
    }
  })
  if (!response.ok) {
    throw new Error(`Data API error: ${response.status} ${await response.text()}`)
  }
  const data = await response.json()
  if (Array.isArray(data) && data.length > 0) {
    return { duplicate: true, existingTranslation: data[0]?.translation ?? null }
  }
  return { duplicate: false, existingTranslation: null }
}

export async function addWord({ text, translation, language = 'Dutch' }) {
  await requireSession()
  const wordId = crypto.randomUUID()
  const cardId = crypto.randomUUID()
  const createdAt = new Date().toISOString()
  const normalizedText = text.trim()
  const normalizedTranslation = translation?.trim() ?? ''
  const duplicateCheck = await checkWordDuplicate(normalizedText, language)
  if (duplicateCheck.duplicate) {
    return { duplicate: true, existingTranslation: duplicateCheck.existingTranslation }
  }
    const word = {
        id: wordId,
        text: normalizedText,
        language,
        translation: normalizedTranslation || null,
        chapter: null,
        group_name: null,
        notes: null,
        created_at: createdAt
    }
  const card = {
    id: cardId,
    word_id: wordId,
    due_at: createdAt,
    interval_days: 0,
    ease: 2.5,
    reps: 0,
    lapses: 0
  }
  if (authToken) {
    const wordResponse = await fetch(`${DATA_API_URL}/words`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        authorization: `Bearer ${authToken}`,
        'content-type': 'application/json',
        prefer: 'return=representation'
      },
      body: JSON.stringify(word)
    })
    if (!wordResponse.ok) {
      throw new Error(`Data API error: ${wordResponse.status} ${await wordResponse.text()}`)
    }
    const cardResponse = await fetch(`${DATA_API_URL}/cards`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        authorization: `Bearer ${authToken}`,
        'content-type': 'application/json',
        prefer: 'return=representation'
      },
      body: JSON.stringify(card)
    })
    if (!cardResponse.ok) {
      throw new Error(`Data API error: ${cardResponse.status} ${await cardResponse.text()}`)
    }
  } else {
    const wordResult = await client.from('words').insert([word]).select('id')
    if (wordResult.error) {
      throw new Error(wordResult.error.message)
    }
    const cardResult = await client.from('cards').insert([card]).select('id')
    if (cardResult.error) {
      throw new Error(cardResult.error.message)
    }
  }

  return {
    wordId,
    cardId,
    createdAt,
    language,
    duplicate: false,
    existingTranslation: null
  }
}

export async function generateSentence({
  word,
  translation,
  sourceLanguage,
  targetLanguage,
  concept
}) {
  const response = await fetch(`${AUTH_SERVER_URL}/ai/generate-sentence`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({
      word,
      translation,
      source_language: sourceLanguage,
      target_language: targetLanguage,
      concept
    })
  })
  if (!response.ok) {
    throw new Error(`AI error: ${response.status} ${await response.text()}`)
  }
  return response.json()
}

export async function addConcept({ name }) {
  await requireSession()
  if (!authToken) {
    throw new Error('You must be signed in to create a concept.')
  }
  const conceptId = crypto.randomUUID()
  const createdAt = new Date().toISOString()
  const payloadName = name.trim()
  if (!payloadName) {
    throw new Error('Concept cannot be empty.')
  }
  const response = await fetch(`${DATA_API_URL}/concepts`, {
    method: 'POST',
    headers: {
      accept: 'application/json',
      authorization: `Bearer ${authToken}`,
      'content-type': 'application/json',
      prefer: 'return=representation'
    },
    body: JSON.stringify({
      id: conceptId,
      name: payloadName,
      created_at: createdAt
    })
  })
  if (!response.ok) {
    throw new Error(`Data API error: ${response.status} ${await response.text()}`)
  }
  return { id: conceptId, name: payloadName, createdAt }
}

export async function generateQuestion({
  word,
  translation,
  sourceLanguage,
  targetLanguage,
  concept
}) {
  const response = await fetch(`${AUTH_SERVER_URL}/ai/generate-question`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({
      word,
      translation,
      source_language: sourceLanguage,
      target_language: targetLanguage,
      concept
    })
  })
  if (!response.ok) {
    throw new Error(`AI error: ${response.status} ${await response.text()}`)
  }
  return response.json()
}

export async function gradeSentence({ word, targetLanguage, userSentence, question, concept }) {
  const response = await fetch(`${AUTH_SERVER_URL}/ai/grade-sentence`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({
      word,
      target_language: targetLanguage,
      user_sentence: userSentence,
      question,
      concept
    })
  })
  if (!response.ok) {
    throw new Error(`AI error: ${response.status} ${await response.text()}`)
  }
  return response.json()
}

export async function startLogin() {
  authState = 'redirecting'
  const callbackURL = isTauri && isIos ? IOS_REDIRECT_URI : REDIRECT_URI
  if (isTauri && isIos) {
    const result = await client.auth.signIn.social({
      provider: 'google',
      callbackURL,
      disableRedirect: true
    })
    const url = result?.data?.url ?? result?.url
    if (!url) {
      throw new Error('No OAuth URL returned')
    }
    await open(url)
    return
  }
  await client.auth.signIn.social({
    provider: 'google',
    callbackURL
  })
}

export async function signInEmail(email, password) {
  authState = 'redirecting'
  const response = await fetch(`${AUTH_SERVER_URL}/auth/sign-in`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ email, password })
  })
  const result = await response.json()
  if (typeof window !== 'undefined') {
    window.__leAuthLastSignIn = result
  }
  if (!response.ok || result?.error) {
    throw new Error(result?.error?.message ?? result?.error ?? 'Sign-in failed')
  }
  if (!result?.access_token) {
    throw new Error('Sign-in failed')
  }
  if (result?.access_token) authToken = result.access_token
  authState = authToken ? 'signed_in' : 'signed_out'
  await refreshAuthState()
}

export async function signUpEmail(email, password, name) {
  authState = 'redirecting'
  const response = await fetch(`${AUTH_SERVER_URL}/auth/sign-up`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ email, password, name })
  })
  const result = await response.json()
  if (typeof window !== 'undefined') {
    window.__leAuthLastSignUp = result
  }
  if (!response.ok || result?.error) {
    throw new Error(result?.error?.message ?? result?.error ?? 'Sign-up failed')
  }
  if (!result?.access_token) {
    throw new Error('Sign-up failed')
  }
  if (result?.access_token) authToken = result.access_token
  authState = authToken ? 'signed_in' : 'signed_out'
  await refreshAuthState()
}

export async function testDataApiRequest() {
  const result = await client.from('words').select('*').order('id', { ascending: false }).limit(1)
  if (result.error) {
    throw new Error(result.error.message)
  }
  authState = 'signed_in'
  return result.data
}
